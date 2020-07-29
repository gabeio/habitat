use super::{get_key_revisions,
            mk_key_filename,
            parse_name_with_rev,
            ring_key::RingKey,
            HabitatKey,
            KeyType,
            SECRET_SYM_KEY_SUFFIX};
use crate::{crypto::{hash,
                     keys::{Permissioned,
                            ToKeyString}},
            error::{Error,
                    Result},
            fs::AtomicWriter};
use sodiumoxide::crypto::secretbox::Key as SymSecretKey;
use std::{convert::TryFrom,
          io::Write,
          path::{Path,
                 PathBuf}};

pub struct KeyCache(PathBuf);

impl KeyCache {
    pub fn new<P>(path: P) -> Self
        where P: Into<PathBuf>
    {
        KeyCache(path.into())
    }

    /// Ensure that the directory backing the cache exists on disk.
    pub fn setup(&self) -> Result<()> {
        if !self.0.is_dir() {
            std::fs::create_dir_all(&self.0)?;
        }
        Ok(())
    }

    pub fn write_ring_key(&self, key: &RingKey) -> Result<()> { self.maybe_write_key(key) }

    /// Returns the full path to the file of the given `RingKey`.
    pub fn ring_key_cached_path(&self, key: &RingKey) -> Result<PathBuf> {
        let path = self.path_in_cache(&key);
        if !path.is_file() {
            return Err(Error::CryptoError(format!("No cached ring key found at \
                                                   {}",
                                                  path.display())));
        }
        Ok(path)
    }

    /// Note: name is just the name, not the name + revision
    pub fn latest_ring_key_revision(&self, name: &str) -> Result<RingKey> {
        let mut all = self.get_pairs_for(name)?;
        match all.len() {
            0 => {
                let msg = format!("No revisions found for {} sym key", name);
                Err(Error::CryptoError(msg))
            }
            _ => Ok(all.remove(0)),
        }
    }

    /// Provides the path at which this file would be found in the
    /// cache, if it exists.
    ///
    /// No existence claim is implied by this method!
    fn path_in_cache<K>(&self, key: K) -> PathBuf
        where K: AsRef<Path>
    {
        self.0.join(key.as_ref())
    }

    /// Write the given key into the cache. If the key already exists,
    /// and the content has the same hash value, nothing will be
    /// done. If the file exists and it has *different* content, an
    /// Error is returned.
    fn maybe_write_key<K>(&self, key: &K) -> Result<()>
        where K: AsRef<Path> + ToKeyString + Permissioned
    {
        let keyfile = self.path_in_cache(&key);
        let content = key.to_key_string()?;

        if keyfile.is_file() {
            let existing_hash = hash::hash_file(&keyfile)?;
            let new_hash = hash::hash_string(&content);
            if existing_hash != new_hash {
                let msg = format!("Existing key file {} found but new version hash is different, \
                                   failing to write new file over existing. (existing = {}, \
                                   incoming = {})",
                                  keyfile.display(),
                                  existing_hash,
                                  new_hash);
                return Err(Error::CryptoError(msg));
            }
        } else {
            // Technically speaking, this probably doesn't really need
            // to be an atomic write process, since we just tested
            // that the file doesn't currently exist. It does,
            // however, bundle up writing with platform-independent
            // permission setting, which is *super* convenient.
            let w = AtomicWriter::new_with_permissions(&keyfile, K::permissions())?;
            w.with_writer(|f| f.write_all(content.as_ref()))?;
        }
        Ok(())
    }
}

// for RingKey implementations
impl KeyCache {
    // TODO (CM): Really, we just need to find all the files that
    // pertain to this named key, sort them by revision, and then read
    // the last one into a RingKey.

    fn get_pairs_for(&self, name: &str) -> Result<Vec<RingKey>> {
        let revisions = get_key_revisions(name, &self.0, None, KeyType::Sym)?;
        let mut key_pairs = Vec::new();
        for name_with_rev in &revisions {
            debug!("Attempting to read key name_with_rev {} for {}",
                   name_with_rev, name);
            let kp = self.get_pair_for(name_with_rev)?;
            key_pairs.push(kp);
        }
        Ok(key_pairs)
    }

    fn get_pair_for(&self, name_with_rev: &str) -> Result<RingKey> {
        let (name, rev) = parse_name_with_rev(&name_with_rev)?;

        // reading the secret key here is really about parsing the base64 bytes into an actual key.
        // That truly should be part of the "from string"
        let sk = match Self::get_secret_key(name_with_rev, &self.0) {
            Ok(k) => Some(k),
            Err(e) => {
                let msg = format!("No secret keys found for name_with_rev {}: {}",
                                  name_with_rev, e);
                return Err(Error::CryptoError(msg));
            }
        };
        Ok(RingKey::from_raw(name, rev, sk))
    }

    fn get_secret_key(key_with_rev: &str, cache_key_path: &Path) -> Result<SymSecretKey> {
        let secret_keyfile = mk_key_filename(cache_key_path, key_with_rev, SECRET_SYM_KEY_SUFFIX);
        match SymSecretKey::from_slice(HabitatKey::try_from(&secret_keyfile)?.as_ref()) {
            Some(sk) => Ok(sk),
            None => {
                Err(Error::CryptoError(format!("Can't read sym secret key \
                                                for {}",
                                               key_with_rev)))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::crypto::test_support::*;

    static VALID_KEY: &str = "ring-key-valid-20160504220722.sym.key";
    static VALID_NAME_WITH_REV: &str = "ring-key-valid-20160504220722";

    #[test]
    fn get_pairs_for() {
        let (cache, _dir) = new_cache();

        let pairs = cache.get_pairs_for("beyonce").unwrap();
        assert_eq!(pairs.len(), 0);

        cache.write_ring_key(&RingKey::new("beyonce")).unwrap();
        let pairs = cache.get_pairs_for("beyonce").unwrap();
        assert_eq!(pairs.len(), 1);

        wait_1_sec(); // ensure new revision
                      // will be different.
        cache.write_ring_key(&RingKey::new("beyonce")).unwrap();

        let pairs = cache.get_pairs_for("beyonce").unwrap();
        assert_eq!(pairs.len(), 2);

        // We should not include another named key in the count
        cache.write_ring_key(&RingKey::new("jayz")).unwrap();
        let pairs = cache.get_pairs_for("beyonce").unwrap();
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn latest_cached_revision_single() {
        let (cache, _dir) = new_cache();

        let key = RingKey::new("beyonce");
        cache.write_ring_key(&key).unwrap();

        let latest = cache.latest_ring_key_revision("beyonce").unwrap();
        assert_eq!(latest.name(), key.name());
        assert_eq!(latest.revision(), key.revision());
    }

    #[test]
    fn latest_cached_revision_multiple() {
        let (cache, _dir) = new_cache();

        let k1 = RingKey::new("beyonce");
        cache.write_ring_key(&k1).unwrap();

        wait_1_sec();

        let k2 = RingKey::new("beyonce");
        cache.write_ring_key(&k2).unwrap();

        assert_eq!(k1.name(), k2.name());
        assert_ne!(k1.revision(), k2.revision());

        let latest = cache.latest_ring_key_revision("beyonce").unwrap();
        assert_eq!(latest.name(), k2.name());
        assert_eq!(latest.revision(), k2.revision());
    }

    #[test]
    #[should_panic(expected = "No revisions found for")]
    fn latest_cached_revision_nonexistent() {
        let (cache, _dir) = new_cache();
        cache.latest_ring_key_revision("nope-nope").unwrap();
    }

    #[test]
    fn get_pair_for() {
        let (cache, _dir) = new_cache();
        let k1 = RingKey::new("beyonce");
        cache.write_ring_key(&k1).unwrap();

        wait_1_sec();

        let k2 = RingKey::new("beyonce");
        cache.write_ring_key(&k2).unwrap();

        let k1_fetched = cache.get_pair_for(&k1.name_with_rev()).unwrap();
        assert_eq!(k1.name(), k1_fetched.name());
        assert_eq!(k1.revision(), k1_fetched.revision());

        let k2_fetched = cache.get_pair_for(&k2.name_with_rev()).unwrap();
        assert_eq!(k2.name(), k2_fetched.name());
        assert_eq!(k2.revision(), k2_fetched.revision());
    }

    #[test]
    #[should_panic(expected = "No secret keys found for name_with_rev")]
    fn get_pair_for_nonexistent() {
        let (cache, _dir) = new_cache();
        cache.get_pair_for("nope-nope-20160405144901").unwrap();
    }

    #[test]
    fn writing_ring_key() {
        let (cache, dir) = new_cache();

        let content = fixture_as_string(&format!("keys/{}", VALID_KEY));
        let new_key_file = dir.path().join(VALID_KEY);
        assert_eq!(new_key_file.is_file(), false);

        let key: RingKey = content.parse().unwrap();
        assert_eq!(key.name_with_rev(), VALID_NAME_WITH_REV);
        cache.write_ring_key(&key).unwrap();
        assert!(new_key_file.is_file());

        let new_content = std::fs::read_to_string(new_key_file).unwrap();
        assert_eq!(new_content, content);
    }

    #[test]
    fn write_key_with_existing_identical() {
        let (cache, dir) = new_cache();
        let content = fixture_as_string(&format!("keys/{}", VALID_KEY));
        let new_key_file = dir.path().join(VALID_KEY);

        // install the key into the cache
        std::fs::copy(fixture(&format!("keys/{}", VALID_KEY)), &new_key_file).unwrap();

        let key: RingKey = content.parse().unwrap();
        cache.write_ring_key(&key).unwrap();
        assert_eq!(key.name_with_rev(), VALID_NAME_WITH_REV);
        assert!(new_key_file.is_file());
    }

    #[test]
    #[should_panic(expected = "Existing key file")]
    fn write_key_exists_but_hashes_differ() {
        let (cache, dir) = new_cache();
        let old_content = fixture_as_string("keys/ring-key-valid-20160504220722.sym.key");

        std::fs::write(dir.path().join("ring-key-valid-20160504220722.sym.key"),
                       &old_content).unwrap();

        #[rustfmt::skip]
        let new_content = "SYM-SEC-1\nring-key-valid-20160504220722\n\nkA+c03Ly5qEoOZIjJ5zCD2vHI05pAW59PfCOb8thmZw=";

        assert_ne!(old_content, new_content);

        let new_key: RingKey = new_content.parse().unwrap();
        // this should fail
        cache.write_ring_key(&new_key).unwrap();
    }

    // Old tests... not fully converting over to new implementation
    // yet because I think the function won't be sticking around very
    // long.

    // #[test]
    // fn cached_path() {
    //     let (cache, dir) = new_cache();
    //     fs::copy(fixture(&format!("keys/{}", VALID_KEY)),
    //              dir.path().join(VALID_KEY)).unwrap();

    //     let result = cache.ring_key_cached_path(VALID_NAME_WITH_REV).unwrap();
    //     assert_eq!(result, cache.path().join(VALID_KEY));
    // }

    // #[test]
    // #[should_panic(expected = "No secret key found at")]
    // fn get_secret_key_path_nonexistent() {
    //     let (cache, _dir) = new_cache();
    //     cache.ring_key_cached_path(VALID_NAME_WITH_REV).unwrap();
    // }
}
