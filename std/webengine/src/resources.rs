// SPDX-License-Identifier: Apache-2.0
//! Servo's built-in pages, with our own certificate and network error pages (browser UI5). Servo's stock
//! certificate page offers "Allow certificate temporarily"; ours has no button and tells NEOWEB, which shows
//! br2-cert-error (the override only with Settings' switch on, B10) or br2-offline.

use std::path::PathBuf;
use std::sync::LazyLock;

use servo::resources::{Resource, ResourceReaderMethods};
use servo_default_resources::DefaultResourceReader;

/// The start of the console line our error pages write: `neoweb:error <token> cert|net <base64 cert or -> <reason>`.
pub const ERROR_MARK: &str = "neoweb:error ";

/// A random token per engine run, written into our error pages: a site can't fake the console line without it.
pub static TOKEN: LazyLock<String> = LazyLock::new(|| {
    let mut b = [0u8; 16];
    let _ = getrandom03::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
});

struct NeoResources;

impl ResourceReaderMethods for NeoResources {
    fn read(&self, res: Resource) -> Vec<u8> {
        let page = match res {
            Resource::BadCertHTML => include_str!("../pages/errors/cert.html"),
            Resource::NetErrorHTML => include_str!("../pages/errors/net.html"),
            other => return DefaultResourceReader.read(other),
        };
        page.replace("${token}", &TOKEN).into_bytes()
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }
}

servo::submit_resource_reader!(&NeoResources);
