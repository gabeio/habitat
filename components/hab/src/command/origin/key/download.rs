use crate::{api_client::{BuilderAPIClient,
                         Client},
            common::{self,
                     command::package::install::{RETRIES,
                                                 RETRY_WAIT},
                     ui::{Status,
                          UIWriter,
                          UI}},
            error::{Error,
                    Result},
            PRODUCT,
            VERSION};
use habitat_core::crypto::keys::{KeyCache,
                                 NamedRevision};
use retry::delay;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn start(ui: &mut UI,
                   bldr_url: &str,
                   origin: &str,
                   revision: Option<&str>,
                   secret: bool,
                   encryption: bool,
                   token: Option<&str>,
                   cache: &Path)
                   -> Result<()> {
    let api_client = Client::new(bldr_url, PRODUCT, VERSION, None)?;

    if secret {
        handle_secret(ui, &api_client, origin, token, cache).await
    } else if encryption {
        handle_encryption(ui, &api_client, origin, token, cache).await
    } else {
        handle_public(ui, &api_client, origin, revision, token, cache).await
    }
}

async fn handle_public(ui: &mut UI,
                       api_client: &BuilderAPIClient,
                       origin: &str,
                       revision: Option<&str>,
                       token: Option<&str>,
                       cache: &Path)
                       -> Result<()> {
    match revision {
        Some(revision) => {
            let named_revision = format!("{}-{}", origin, revision).parse()?;
            ui.begin(format!("Downloading public origin key {}", named_revision))?;
            match download_key(ui, api_client, &named_revision, token, cache).await {
                Ok(()) => {
                    let msg = format!("Download of {} public origin key completed.",
                                      named_revision);
                    ui.end(msg)?;
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        None => {
            ui.begin(format!("Downloading public origin keys for {}", origin))?;
            match api_client.show_origin_keys(origin).await {
                Ok(ref keys) if keys.is_empty() => {
                    ui.end(format!("No public keys for {}.", origin))?;
                    Ok(())
                }
                Ok(keys) => {
                    for key in keys {
                        let named_revision = format!("{}-{}", key.origin, key.revision).parse()?;
                        download_key(ui, api_client, &named_revision, token, cache).await?;
                    }
                    ui.end(format!("Download of {} public origin keys completed.", &origin))?;
                    Ok(())
                }
                Err(e) => Err(Error::from(e)),
            }
        }
    }
}

async fn handle_secret(ui: &mut UI,
                       api_client: &BuilderAPIClient,
                       origin: &str,
                       token: Option<&str>,
                       cache: &Path)
                       -> Result<()> {
    if token.is_none() {
        ui.end("No auth token found. You must pass a token to download secret keys.")?;
        return Ok(());
    }

    ui.begin(format!("Downloading secret origin keys for {}", origin))?;
    download_secret_key(ui, &api_client, origin, token.unwrap(), cache).await?; // unwrap is safe because we already checked it above
    ui.end(format!("Download of {} secret origin keys completed.", &origin))?;
    Ok(())
}

async fn handle_encryption(ui: &mut UI,
                           api_client: &BuilderAPIClient,
                           origin: &str,
                           token: Option<&str>,
                           cache: &Path)
                           -> Result<()> {
    if token.is_none() {
        ui.end("No auth token found. You must pass a token to download secret keys.")?;
        return Ok(());
    }

    ui.begin(format!("Downloading public encryption origin key for {}", origin))?;
    download_public_encryption_key(ui, &api_client, origin, token.unwrap(), cache).await?; // unwrap is safe because we already checked it above
    ui.end(format!("Download of {} public encryption keys completed.", &origin))?;
    Ok(())
}

pub async fn download_public_encryption_key(ui: &mut UI,
                                            api_client: &BuilderAPIClient,
                                            name: &str,
                                            token: &str,
                                            cache: &Path)
                                            -> Result<()> {
    retry::retry_future!(delay::Fixed::from(RETRY_WAIT).take(RETRIES), async {
        ui.status(Status::Downloading, "latest public encryption key")?;
        let key_path =
            api_client.fetch_origin_public_encryption_key(name, token, cache, ui.progress())
                      .await?;
        ui.status(Status::Cached,
                  key_path.file_name().unwrap().to_str().unwrap() /* lol */)?;
        Ok::<_, Error>(())
    }).await
      .map_err(|_| {
          Error::from(common::error::Error::DownloadFailed(format!("We tried {} times but could \
                                                                    not download the latest \
                                                                    public encryption key. \
                                                                    Giving up.",
                                                                   RETRIES,)))
      })
}

async fn download_secret_key(ui: &mut UI,
                             api_client: &BuilderAPIClient,
                             name: &str,
                             token: &str,
                             cache: &Path)
                             -> Result<()> {
    retry::retry_future!(delay::Fixed::from(RETRY_WAIT).take(RETRIES), async {
        ui.status(Status::Downloading, "latest secret key")?;
        let key_path = api_client.fetch_secret_origin_key(name, token, cache, ui.progress())
                                 .await?;
        ui.status(Status::Cached,
                  key_path.file_name().unwrap().to_str().unwrap() /* lol */)?;
        Ok::<_, Error>(())
    }).await
      .map_err(|_| {
          Error::from(common::error::Error::DownloadFailed(format!("We tried {} times but could \
                                                                    not download the latest \
                                                                    secret origin key. Giving \
                                                                    up.",
                                                                   RETRIES,)))
      })
}

async fn download_key(ui: &mut UI,
                      api_client: &BuilderAPIClient,
                      named_revision: &NamedRevision,
                      token: Option<&str>,
                      cache: &Path)
                      -> Result<()> {
    let cache = KeyCache::new(cache);

    if cache.public_signing_key(named_revision).is_ok() {
        ui.status(Status::Using,
                  &format!("{} in {}", named_revision, cache.as_ref().display()))?;
        Ok(())
    } else {
        retry::retry_future!(delay::Fixed::from(RETRY_WAIT).take(RETRIES), async {
            ui.status(Status::Downloading, named_revision)?;
            api_client.fetch_origin_key(named_revision.name(),
                                        named_revision.revision(),
                                        token,
                                        cache.as_ref(),
                                        ui.progress())
                      .await?;
            ui.status(Status::Cached,
                      &format!("{} to {}", named_revision, cache.as_ref().display()))?;
            Ok::<_, Error>(())
        }).await
          .map_err(|_| {
              Error::from(common::error::Error::DownloadFailed(format!("We tried {} times but \
                                                                        could not download {} \
                                                                        origin key. Giving up.",
                                                                       RETRIES, named_revision)))
          })
    }
}
