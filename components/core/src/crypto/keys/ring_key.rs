use super::{super::{hash,
                    SECRET_SYM_KEY_SUFFIX,
                    SECRET_SYM_KEY_VERSION},
            get_key_revisions,
            mk_key_filename,
            parse_name_with_rev,
            write_keypair_files,
            HabitatKey,
            KeyPair,
            KeyRevision,
            KeyType,
            PairType,
            TmpKeyfile};
use crate::error::{Error,
                   Result};
use sodiumoxide::{crypto::secretbox::{self,
                                      Key as SymSecretKey},
                  randombytes::randombytes};
use std::{convert::TryFrom,
          fmt,
          fs,
          path::{Path,
                 PathBuf}};

#[derive(Clone, PartialEq)]
pub struct RingKey(KeyPair<(), SymSecretKey>);

// TODO (CM): Incorporate the name/revision of the key?
impl fmt::Debug for RingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "RingKey") }
}

impl RingKey {
    /// Generate a new `RingKey` for the given name. Creates a new
    /// key, but does not write anything to the filesystem.
    pub fn new(name: &str) -> Self {
        let revision = KeyRevision::new();
        let secret_key = secretbox::gen_key();
        RingKey(KeyPair::new(name.to_string(), revision, Some(()), Some(secret_key)))
    }

    // Simple helper to deal with the indirection to the inner
    // KeyPair struct. Not ultimately sure if this should be kept.
    pub fn name_with_rev(&self) -> String { self.0.name_with_rev() }

    pub fn get_latest_pair_for<P: AsRef<Path> + ?Sized>(name: &str,
                                                        cache_key_path: &P)
                                                        -> Result<Self> {
        let mut all = Self::get_pairs_for(name, cache_key_path)?;
        match all.len() {
            0 => {
                let msg = format!("No revisions found for {} sym key", name);
                Err(Error::CryptoError(msg))
            }
            _ => Ok(all.remove(0)),
        }
    }

    /// Returns the full path to the secret sym key given a key name with revision.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```
    /// extern crate habitat_core;
    /// extern crate tempfile;
    ///
    /// use habitat_core::crypto::RingKey;
    /// use std::fs::File;
    /// use tempfile::Builder;
    ///
    /// let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
    /// let secret_file = cache.path().join("beyonce-20160504220722.sym.key");
    /// let _ = File::create(&secret_file).unwrap();
    ///
    /// let path = RingKey::get_secret_key_path("beyonce-20160504220722", cache.path()).unwrap();
    /// assert_eq!(path, secret_file);
    /// ```
    ///
    /// # Errors
    ///
    /// * If no file exists at the the computed file path
    pub fn get_secret_key_path<P: AsRef<Path> + ?Sized>(key_with_rev: &str,
                                                        cache_key_path: &P)
                                                        -> Result<PathBuf> {
        let path = mk_key_filename(cache_key_path.as_ref(), key_with_rev, SECRET_SYM_KEY_SUFFIX);
        if !path.is_file() {
            return Err(Error::CryptoError(format!("No secret key found at {}", path.display())));
        }
        Ok(path)
    }

    /// Encrypts a byte slice of data using a given `RingKey`.
    ///
    /// The return is a `Result` of a tuple of `Vec<u8>` structs, the first being the random nonce
    /// value and the second being the ciphertext.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```
    /// extern crate habitat_core;
    /// extern crate tempfile;
    ///
    /// use habitat_core::crypto::RingKey;
    /// use tempfile::Builder;
    ///
    /// let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
    /// let ring_key = RingKey::new("beyonce");
    ///
    /// let (nonce, ciphertext) = ring_key.encrypt("Guess who?".as_bytes()).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// * If the secret key component of the `RingKey` is not present
    pub fn encrypt(&self, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let key = self.0.secret()?;
        let nonce = secretbox::gen_nonce();
        Ok((nonce.as_ref().to_vec(), secretbox::seal(data, &nonce, &key)))
    }

    /// Decrypts a byte slice of ciphertext using a given nonce value and a `RingKey`.
    ///
    /// The return is a `Result` of a byte vector containing the original, unencrypted data.
    ///
    /// # Examples
    ///
    /// Basic usage
    ///
    /// ```
    /// extern crate habitat_core;
    /// extern crate tempfile;
    ///
    /// use habitat_core::crypto::RingKey;
    /// use tempfile::Builder;
    ///
    /// let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
    /// let ring_key = RingKey::new("beyonce");
    /// let (nonce, ciphertext) = ring_key.encrypt("Guess who?".as_bytes()).unwrap();
    ///
    /// let message = ring_key.decrypt(&nonce, &ciphertext).unwrap();
    /// assert_eq!(message, "Guess who?".to_string().into_bytes());
    /// ```
    ///
    /// # Errors
    ///
    /// * If the secret key component of the `RingKey` is not present
    /// * If the size of the provided nonce data is not the required size
    /// * If the ciphertext was not decryptable given the nonce and symmetric key
    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
        let key = self.0.secret()?;
        let nonce = match secretbox::Nonce::from_slice(&nonce) {
            Some(n) => n,
            None => return Err(Error::CryptoError("Invalid size of nonce".to_string())),
        };
        match secretbox::open(ciphertext, &nonce, &key) {
            Ok(msg) => Ok(msg),
            Err(_) => {
                Err(Error::CryptoError("Secret key and nonce could not \
                                        decrypt ciphertext"
                                                           .to_string()))
            }
        }
    }

    pub fn write_to_cache<P>(&self, cache_dir: P) -> Result<()>
        where P: AsRef<Path>
    {
        let secret_keyfile = mk_key_filename(cache_dir.as_ref(),
                                             self.name_with_rev(),
                                             SECRET_SYM_KEY_SUFFIX);
        debug!("secret sym keyfile = {}", secret_keyfile.display());

        write_keypair_files(None,
                            None,
                            Some(&secret_keyfile),
                            Some(self.to_secret_string()?))
    }

    /// Writes a sym key to the key cache from the contents of a string slice.
    ///
    /// The return is a `Result` of a `String` containing the key's name with revision.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```
    /// extern crate habitat_core;
    /// extern crate tempfile;
    ///
    /// use habitat_core::crypto::{keys::PairType,
    ///                            RingKey};
    /// use tempfile::Builder;
    ///
    /// let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
    /// let content = "SYM-SEC-1
    /// beyonce-20160504220722
    ///
    /// RCFaO84j41GmrzWddxMdsXpGdn3iuIy7Mw3xYrjPLsE=";
    ///
    /// let (pair, pair_type) = RingKey::write_file_from_str(content, cache.path()).unwrap();
    /// assert_eq!(pair_type, PairType::Secret);
    /// assert_eq!(pair.name_with_rev(), "beyonce-20160504220722");
    /// assert!(cache.path()
    ///              .join("beyonce-20160504220722.sym.key")
    ///              .is_file());
    /// ```
    ///
    /// # Errors
    ///
    /// * If there is a key version mismatch
    /// * If the key version is missing
    /// * If the key name with revision is missing
    /// * If the key value (the Bas64 payload) is missing
    /// * If the key file cannot be written to disk
    /// * If an existing key is already installed, but the new content is different from the
    /// existing
    pub fn write_file_from_str<P: AsRef<Path> + ?Sized>(content: &str,
                                                        cache_key_path: &P)
                                                        -> Result<(Self, PairType)> {
        let mut lines = content.lines();
        match lines.next() {
            Some(val) => {
                if val != SECRET_SYM_KEY_VERSION {
                    return Err(Error::CryptoError(format!("Unsupported key version: {}", val)));
                }
            }
            None => {
                let msg = format!("write_sym_key_from_str:1 Malformed sym key string:\n({})",
                                  content);
                return Err(Error::CryptoError(msg));
            }
        };
        let name_with_rev = match lines.next() {
            Some(val) => val,
            None => {
                let msg = format!("write_sym_key_from_str:2 Malformed sym key string:\n({})",
                                  content);
                return Err(Error::CryptoError(msg));
            }
        };
        if lines.nth(1).is_none() {
            let msg = format!("write_sym_key_from_str:3 Malformed sym key string:\n({})",
                              content);
            return Err(Error::CryptoError(msg));
        };
        let secret_keyfile = mk_key_filename(cache_key_path.as_ref(),
                                             &name_with_rev,
                                             SECRET_SYM_KEY_SUFFIX);
        let tmpfile = {
            let mut t = secret_keyfile.clone();
            t.set_file_name(format!("{}.{}",
                                    &secret_keyfile.file_name().unwrap().to_str().unwrap(),
                                    &hex::encode(randombytes(6).as_slice())));
            TmpKeyfile { path: t }
        };

        debug!("Writing temp key file {}", tmpfile.path.display());
        write_keypair_files(None, None, Some(&tmpfile.path), Some(content.to_string()))?;

        if Path::new(&secret_keyfile).is_file() {
            let existing_hash = hash::hash_file(&secret_keyfile)?;
            let new_hash = hash::hash_file(&tmpfile.path)?;
            if existing_hash != new_hash {
                let msg = format!("Existing key file {} found but new version hash is different, \
                                   failing to write new file over existing. ({} = {}, {} = {})",
                                  secret_keyfile.display(),
                                  secret_keyfile.display(),
                                  existing_hash,
                                  tmpfile.path.display(),
                                  new_hash);
                return Err(Error::CryptoError(msg));
            } else {
                // Otherwise, hashes match and we can skip writing over the existing file
                debug!("New content hash matches existing file {} hash, removing temp key file \
                        {}.",
                       secret_keyfile.display(),
                       tmpfile.path.display());
                fs::remove_file(&tmpfile.path)?;
            }
        } else {
            debug!("Moving {} to {}",
                   tmpfile.path.display(),
                   secret_keyfile.display());
            fs::rename(&tmpfile.path, secret_keyfile)?;
        }

        // Now load and return the pair to ensure everything wrote out
        Ok((Self::get_pair_for(&name_with_rev, cache_key_path)?, PairType::Secret))
    }

    ////////////////////////////////////////////////////////////////////////

    // TODO (CM): only public because it's also used in a test in
    // keys.rs... look into better factoring
    pub(crate) fn to_secret_string(&self) -> Result<String> {
        match self.0.secret {
            Some(ref sk) => {
                Ok(format!("{}\n{}\n\n{}",
                           SECRET_SYM_KEY_VERSION,
                           self.name_with_rev(),
                           &base64::encode(&sk[..])))
            }
            None => {
                Err(Error::CryptoError(format!("No secret key present for {}",
                                               self.name_with_rev())))
            }
        }
    }

    fn get_pairs_for<P: AsRef<Path> + ?Sized>(name: &str, cache_key_path: &P) -> Result<Vec<Self>> {
        let revisions = get_key_revisions(name, cache_key_path.as_ref(), None, KeyType::Sym)?;
        let mut key_pairs = Vec::new();
        for name_with_rev in &revisions {
            debug!("Attempting to read key name_with_rev {} for {}",
                   name_with_rev, name);
            let kp = Self::get_pair_for(name_with_rev, cache_key_path)?;
            key_pairs.push(kp);
        }
        Ok(key_pairs)
    }

    fn get_pair_for<P: AsRef<Path> + ?Sized>(name_with_rev: &str,
                                             cache_key_path: &P)
                                             -> Result<Self> {
        let (name, rev) = parse_name_with_rev(&name_with_rev)?;
        let sk = match Self::get_secret_key(name_with_rev, cache_key_path.as_ref()) {
            Ok(k) => Some(k),
            Err(e) => {
                let msg = format!("No secret keys found for name_with_rev {}: {}",
                                  name_with_rev, e);
                return Err(Error::CryptoError(msg));
            }
        };
        Ok(RingKey(KeyPair::new(name, rev, None, sk)))
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
    use super::{super::super::test_support::*,
                *};
    use std::{fs::{self,
                   File},
              io::Read};
    use tempfile::Builder;

    static VALID_KEY: &str = "ring-key-valid-20160504220722.sym.key";
    static VALID_NAME_WITH_REV: &str = "ring-key-valid-20160504220722";

    impl RingKey {
        /// Test-only way of creating a RingKey to e.g., simulate when
        /// a requested key doesn't exist on disk.
        pub fn from_raw(name: String, rev: KeyRevision, secret: Option<SymSecretKey>) -> RingKey {
            RingKey(KeyPair::new(name, rev, Some(()), secret))
        }

        pub fn revision(&self) -> &KeyRevision { &self.0.revision }

        pub fn name(&self) -> &String { &self.0.name }

        // TODO (CM): This really shouldn't exist
        pub fn public(&self) -> crate::error::Result<&()> { self.0.public() }

        // TODO (CM): this should probably be renamed; there's no
        // public key to distinguish it from.
        pub fn secret(&self) -> crate::error::Result<&SymSecretKey> { self.0.secret() }
    }

    // #[test]
    // fn empty_struct() {
    //     let pair = RingKey::new("grohl".to_string(),
    //                             KeyRevision::unchecked("201604051449"),
    //                             None,
    //                             None);

    //     assert_eq!(pair.name(), "grohl");
    //     assert_eq!(pair.revision(), KeyRevision::unchecked("201604051449"));
    //     assert_eq!(pair.name_with_rev(), "grohl-201604051449");

    //     assert_eq!(pair.public, None);
    //     assert!(pair.public().is_err(),
    //             "Empty pair should not have a public key");
    //     assert_eq!(pair.secret, None);
    //     assert!(pair.secret().is_err(),
    //             "Empty pair should not have a secret key");
    // }

    #[test]
    fn generated_ring_pair() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let key = RingKey::new("beyonce");
        key.write_to_cache(cache.path()).unwrap();

        assert_eq!(key.name(), "beyonce");
        assert!(key.public().is_ok(),
                "Generated pair should have an empty public key");
        assert!(key.secret().is_ok(),
                "Generated pair should have a secret key");
        assert!(cache.path()
                     .join(format!("{}.sym.key", key.name_with_rev()))
                     .exists());
    }

    #[test]
    fn get_pairs_for() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let pairs = RingKey::get_pairs_for("beyonce", cache.path()).unwrap();
        assert_eq!(pairs.len(), 0);

        RingKey::new("beyonce").write_to_cache(cache.path())
                               .unwrap();
        let pairs = RingKey::get_pairs_for("beyonce", cache.path()).unwrap();
        assert_eq!(pairs.len(), 1);

        match wait_until_ok(|| {
                  let rpair = RingKey::new("beyonce");
                  rpair.write_to_cache(cache.path())?;
                  Ok(())
              }) {
            Some(pair) => pair,
            None => panic!("Failed to generate another keypair after waiting"),
        };
        let pairs = RingKey::get_pairs_for("beyonce", cache.path()).unwrap();
        assert_eq!(pairs.len(), 2);

        // We should not include another named key in the count
        RingKey::new("jayz").write_to_cache(cache.path()).unwrap();
        let pairs = RingKey::get_pairs_for("beyonce", cache.path()).unwrap();
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn get_pair_for() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let k1 = RingKey::new("beyonce");
        k1.write_to_cache(cache.path()).unwrap();
        let k2 = match wait_until_ok(|| {
                  let rpath = RingKey::new("beyonce");
                  rpath.write_to_cache(cache.path())?;
                  Ok(rpath)
              }) {
            Some(key) => key,
            None => panic!("Failed to generate another keypair after waiting"),
        };

        let p1_fetched = RingKey::get_pair_for(&k1.name_with_rev(), cache.path()).unwrap();
        assert_eq!(k1.name(), p1_fetched.name());
        assert_eq!(k1.revision(), p1_fetched.revision());
        let p2_fetched = RingKey::get_pair_for(&k2.name_with_rev(), cache.path()).unwrap();
        assert_eq!(k2.name(), p2_fetched.name());
        assert_eq!(k2.revision(), p2_fetched.revision());
    }

    #[test]
    #[should_panic(expected = "No secret keys found for name_with_rev")]
    fn get_pair_for_nonexistent() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        RingKey::get_pair_for("nope-nope-20160405144901", cache.path()).unwrap();
    }

    #[test]
    fn get_latest_pair_for_single() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let key = RingKey::new("beyonce");
        key.write_to_cache(cache.path()).unwrap();

        let latest = RingKey::get_latest_pair_for("beyonce", cache.path()).unwrap();
        assert_eq!(latest.name(), key.name());
        assert_eq!(latest.revision(), key.revision());
    }

    #[test]
    fn get_latest_pair_for_multiple() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        RingKey::new("beyonce").write_to_cache(cache.path())
                               .unwrap();
        let k2 = match wait_until_ok(|| {
                  let rpath = RingKey::new("beyonce");
                  rpath.write_to_cache(cache.path())?;
                  Ok(rpath)
              }) {
            Some(key) => key,
            None => panic!("Failed to generate another keypair after waiting"),
        };

        let latest = RingKey::get_latest_pair_for("beyonce", cache.path()).unwrap();
        assert_eq!(latest.name(), k2.name());
        assert_eq!(latest.revision(), k2.revision());
    }

    #[test]
    #[should_panic(expected = "No revisions found for")]
    fn get_latest_pair_for_nonexistent() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        RingKey::get_latest_pair_for("nope-nope", cache.path()).unwrap();
    }

    #[test]
    fn get_secret_key_path() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        fs::copy(fixture(&format!("keys/{}", VALID_KEY)),
                 cache.path().join(VALID_KEY)).unwrap();

        let result = RingKey::get_secret_key_path(VALID_NAME_WITH_REV, cache.path()).unwrap();
        assert_eq!(result, cache.path().join(VALID_KEY));
    }

    #[test]
    #[should_panic(expected = "No secret key found at")]
    fn get_secret_key_path_nonexistent() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        RingKey::get_secret_key_path(VALID_NAME_WITH_REV, cache.path()).unwrap();
    }

    #[test]
    fn encrypt_and_decrypt() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let key = RingKey::new("beyonce");
        key.write_to_cache(cache.path()).unwrap();

        let (nonce, ciphertext) = key.encrypt(b"Ringonit").unwrap();
        let message = key.decrypt(&nonce, &ciphertext).unwrap();
        assert_eq!(message, "Ringonit".to_string().into_bytes());
    }

    #[test]
    #[should_panic(expected = "Secret key is required but not present for")]
    fn encrypt_missing_secret_key() {
        let key = RingKey::from_raw("grohl".to_string(),
                                    KeyRevision::unchecked("201604051449"),
                                    None);

        key.encrypt(b"Not going to go well").unwrap();
    }

    #[test]
    #[should_panic(expected = "Secret key is required but not present for")]
    fn decrypt_missing_secret_key() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let key = RingKey::new("beyonce");
        key.write_to_cache(cache.path()).unwrap();
        let (nonce, ciphertext) = key.encrypt(b"Ringonit").unwrap();

        let missing = RingKey::from_raw("grohl".to_string(),
                                        KeyRevision::unchecked("201604051449"),
                                        None);
        missing.decrypt(&nonce, &ciphertext).unwrap();
    }

    #[test]
    #[should_panic(expected = "Invalid size of nonce")]
    fn decrypt_invalid_nonce_length() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let key = RingKey::new("beyonce");
        key.write_to_cache(cache.path()).unwrap();

        let (_, ciphertext) = key.encrypt(b"Ringonit").unwrap();
        key.decrypt(b"crazyinlove", &ciphertext).unwrap();
    }

    #[test]
    #[should_panic(expected = "Secret key and nonce could not decrypt ciphertext")]
    fn decrypt_invalid_ciphertext() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let key = RingKey::new("beyonce");
        key.write_to_cache(cache.path()).unwrap();

        let (nonce, _) = key.encrypt(b"Ringonit").unwrap();
        key.decrypt(&nonce, b"singleladies").unwrap();
    }

    #[test]
    fn write_file_from_str() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let content = fixture_as_string(&format!("keys/{}", VALID_KEY));
        let new_key_file = cache.path().join(VALID_KEY);

        assert_eq!(new_key_file.is_file(), false);
        let (key, pair_type) = RingKey::write_file_from_str(&content, cache.path()).unwrap();
        assert_eq!(pair_type, PairType::Secret);
        assert_eq!(key.name_with_rev(), VALID_NAME_WITH_REV);
        assert!(new_key_file.is_file());

        let new_content = {
            let mut new_content_file = File::open(new_key_file).unwrap();
            let mut new_content = String::new();
            new_content_file.read_to_string(&mut new_content).unwrap();
            new_content
        };

        assert_eq!(new_content, content);
    }

    #[test]
    fn write_file_from_str_with_existing_identical() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let content = fixture_as_string(&format!("keys/{}", VALID_KEY));
        let new_key_file = cache.path().join(VALID_KEY);

        // install the key into the cache
        fs::copy(fixture(&format!("keys/{}", VALID_KEY)), &new_key_file).unwrap();

        let (key, pair_type) = RingKey::write_file_from_str(&content, cache.path()).unwrap();
        assert_eq!(pair_type, PairType::Secret);
        assert_eq!(key.name_with_rev(), VALID_NAME_WITH_REV);
        assert!(new_key_file.is_file());
    }

    #[test]
    #[should_panic(expected = "Unsupported key version")]
    fn write_file_from_str_unsupported_version() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let content = fixture_as_string("keys/ring-key-invalid-version-20160504221247.sym.key");

        RingKey::write_file_from_str(&content, cache.path()).unwrap();
    }

    #[test]
    #[should_panic(expected = "write_sym_key_from_str:1 Malformed sym key string")]
    fn write_file_from_str_missing_version() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();

        RingKey::write_file_from_str("", cache.path()).unwrap();
    }

    #[test]
    #[should_panic(expected = "write_sym_key_from_str:2 Malformed sym key string")]
    fn write_file_from_str_missing_name() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();

        RingKey::write_file_from_str("SYM-SEC-1\n", cache.path()).unwrap();
    }

    #[test]
    #[should_panic(expected = "write_sym_key_from_str:3 Malformed sym key string")]
    fn write_file_from_str_missing_key() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();

        RingKey::write_file_from_str("SYM-SEC-1\nim-in-trouble-123\n", cache.path()).unwrap();
    }

    #[test]
    #[should_panic(expected = "Existing key file")]
    fn write_file_from_str_key_exists_but_hashes_differ() {
        let cache = Builder::new().prefix("key_cache").tempdir().unwrap();
        let key = fixture("keys/ring-key-valid-20160504220722.sym.key");
        fs::copy(key,
                 cache.path().join("ring-key-valid-20160504220722.sym.key")).unwrap();

        RingKey::write_file_from_str("SYM-SEC-1\nring-key-valid-20160504220722\n\nsomething",
                                     cache.path()).unwrap();
    }
}
