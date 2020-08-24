use std::path::Path;

use crate::{common::ui::{UIWriter,
                         UI},
            hcore::crypto::RingKey};

use crate::error::Result;

pub fn start(ui: &mut UI, ring: &str, cache: &Path) -> Result<()> {
    ui.begin(format!("Generating ring key for {}", &ring))?;
    let pair = RingKey::new(ring);
    pair.to_pair_files(cache)?;
    ui.end(format!("Generated ring key pair {}.", &pair.name_with_rev()))?;
    Ok(())
}
