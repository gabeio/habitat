use crate::{api_client::{self,
                         retry_builder_api,
                         BuilderAPIClient,
                         Client,
                         API_RETRIES,
                         API_RETRY_WAIT},
            common::{self,
                     ui::{Status,
                          UIWriter,
                          UI}},
            error::{Error,
                    Result},
            hcore::crypto::SigKeyPair,
            PRODUCT,
            VERSION};
use reqwest::StatusCode;
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
            let nwr = format!("{}-{}", origin, revision);
            ui.begin(format!("Downloading public origin key {}", &nwr))?;
            match download_key(ui, api_client, &nwr, origin, revision, token, cache).await {
                Ok(()) => {
                    let msg = format!("Download of {} public origin key completed.", nwr);
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
                        let nwr = format!("{}-{}", key.origin, key.revision);
                        download_key(ui,
                                     api_client,
                                     &nwr,
                                     &key.origin,
                                     &key.revision,
                                     token,
                                     cache).await?;
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
    if let Err(e) =
        retry_builder_api!(None, async {
            ui.status(Status::Downloading, "latest public encryption key")?;
            let key_path =
                api_client.fetch_origin_public_encryption_key(name, token, cache, ui.progress())
                          .await?;
            ui.status(Status::Cached,
                      key_path.file_name().unwrap().to_str().unwrap() /* lol */)?;
            Ok::<_, habitat_api_client::error::Error>(())
        }).await
    {
        return Err(common::error::Error::DownloadFailed(format!("When suitable, we try once \
                                                                 then re-attempt {} times \
                                                                 with a back-off algorithm. \
                                                                 Unfortunately, it seems we \
                                                                 still could not download the \
                                                                 latest encryption key. (Some \
                                                                 HTTP error conditions are \
                                                                 not practically worth \
                                                                 retrying) - last error: {}.",
                                                                API_RETRIES, e)).into());
    }
    Ok(())
}

async fn download_secret_key(ui: &mut UI,
                             api_client: &BuilderAPIClient,
                             name: &str,
                             token: &str,
                             cache: &Path)
                             -> Result<()> {
    if let Err(e) = retry_builder_api!(None, async {
                        ui.status(Status::Downloading, "latest secret key")?;
                        let key_path =
                            api_client.fetch_secret_origin_key(name, token, cache, ui.progress())
                                      .await?;
                        ui.status(Status::Cached,
                                  key_path.file_name().unwrap().to_str().unwrap() /* lol */)?;
                        Ok::<_, habitat_api_client::error::Error>(())
                    }).await
    {
        return Err(common::error::Error::DownloadFailed(format!("When suitable, we try once \
                                                                 then re-attempt {} times \
                                                                 with a back-off algorithm. \
                                                                 Unfortunately, it seems we \
                                                                 still could not download the \
                                                                 latest secret origin key. \
                                                                 (Some HTTP error conditions \
                                                                 are not practically worth \
                                                                 retrying) - last error: {}.",
                                                                API_RETRIES, e)).into());
    }
    Ok(())
}

async fn download_key(ui: &mut UI,
                      api_client: &BuilderAPIClient,
                      nwr: &str,
                      name: &str,
                      rev: &str,
                      token: Option<&str>,
                      cache: &Path)
                      -> Result<()> {
    if SigKeyPair::get_public_key_path(&nwr, &cache).is_ok() {
        ui.status(Status::Using, &format!("{} in {}", nwr, cache.display()))?;
        Ok(())
    } else {
        if let Err(e) = retry_builder_api!(None, async {
                            ui.status(Status::Downloading, &nwr)?;
                            api_client.fetch_origin_key(name, rev, token, cache, ui.progress())
                                      .await?;
                            ui.status(Status::Cached, &format!("{} to {}", nwr, cache.display()))?;
                            Ok::<_, habitat_api_client::error::Error>(())
                        }).await
        {
            return Err(common::error::Error::DownloadFailed(format!("When suitable, we try once \
                                                                     then re-attempt {} times \
                                                                     with a back-off algorithm. \
                                                                     Unfortunately, it seems we \
                                                                     still could not download the \
                                                                     {}/{} origin key. (Some \
                                                                     HTTP error conditions are \
                                                                     not practically worth \
                                                                     retrying) - last error: {}.",
                                                                     API_RETRIES, &name, &rev,
                                                                     e)).into());
        }
        Ok(())
    }
}
