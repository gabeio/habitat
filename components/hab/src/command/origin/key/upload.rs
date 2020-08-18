use std::path::Path;

use super::get_name_with_rev;
use crate::{api_client::{self,
                         retry_builder_api,
                         Client,
                         API_RETRIES,
                         API_RETRY_WAIT},
            common::{error::{APIFailure,
                             Error as CommonError},
                     ui::{Status,
                          UIWriter,
                          UI}},
            error::Result,
            hcore::crypto::{keys::parse_name_with_rev,
                            PUBLIC_SIG_KEY_VERSION,
                            SECRET_SIG_KEY_VERSION},
            PRODUCT,
            VERSION};
use reqwest::StatusCode;

pub async fn start(ui: &mut UI,
                   bldr_url: &str,
                   token: &str,
                   public_keyfile: &Path,
                   secret_keyfile: Option<&Path>)
                   -> Result<()> {
    let api_client = Client::new(bldr_url, PRODUCT, VERSION, None)?;
    ui.begin(format!("Uploading public origin key {}", public_keyfile.display()))?;

    let name_with_rev = get_name_with_rev(&public_keyfile, PUBLIC_SIG_KEY_VERSION)?;
    let (name, rev) = parse_name_with_rev(&name_with_rev)?;

    {
        if let Err(e) =
            retry_builder_api!(None, async {
                ui.status(Status::Uploading, public_keyfile.display())?;
                match api_client.put_origin_key(&name, &rev, public_keyfile, token, ui.progress())
                                .await
                {
                    Ok(()) => ui.status(Status::Uploaded, &name_with_rev)?,
                    Err(api_client::Error::APIError(StatusCode::CONFLICT, _)) => {
                        ui.status(Status::Using,
                                  format!("public key revision {} which already exists in the \
                                           depot",
                                          &name_with_rev))?;
                    }
                    Err(err) => return Err(err),
                }
                Ok::<_, habitat_api_client::error::Error>(())
            }).await
        {
            return Err(CommonError::BuilderAPITransferError(APIFailure::key_upload_failed(API_RETRIES, &name, &rev, e.into())).into());
        }
    }

    ui.end(format!("Upload of public origin key {} complete.", &name_with_rev))?;

    if let Some(secret_keyfile) = secret_keyfile {
        let name_with_rev = get_name_with_rev(&secret_keyfile, SECRET_SIG_KEY_VERSION)?;
        let (name, rev) = parse_name_with_rev(&name_with_rev)?;
        if let Err(e) =
            retry_builder_api!(None, async {
                ui.status(Status::Uploading, secret_keyfile.display())?;
                api_client.put_origin_secret_key(&name, &rev, secret_keyfile, token, ui.progress())
                          .await
            }).await
        {
            return Err(CommonError::BuilderAPITransferError(APIFailure::key_upload_failed(API_RETRIES, &name, &rev, e.into())).into());
        }
        ui.status(Status::Uploaded, &name_with_rev)?;
        ui.end(format!("Upload of secret origin key {} complete.", &name_with_rev))?;
    }
    Ok(())
}
