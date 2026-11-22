// SPDX-License-Identifier: Apache-2.0
//! Site info (browser UI5, `br2-siteinfo`): the TLS version and certificate of the shown page's site.
//! shortcut: a second connection to the site, not the page's own; showing that exact handshake needs a Servo patch
//! that hands `TlsSecurityInfo` to the embedder (it reaches only devtools today).

use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use neoweb_proto::Event;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, ProtocolVersion, RootCertStore};

use crate::downloads::Post;

/// A check is running: one at a time (review: P.I.P.S. sockets take no timeouts, so a site that never answers its
/// handshake would keep each thread; shortcut: that one thread stays blocked until the socket fails).
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Answer [`neoweb_proto::Command::SiteInfo`] for `url` on a thread: `cookies` is the site's cookie count.
pub fn site_info(url: &url::Url, cookies: u32, post: Post) {
    // NEOWEB drops an answer for a page it no longer shows
    let asked = url.host_str().unwrap_or("").to_owned();
    let answer =
        move |tls: &str, summary: neoweb_core::safety::CertSummary, error: &str| Event::SiteInfo {
            host: asked.clone(),
            tls: tls.into(),
            issued_to: summary.issued_to,
            issued_by: summary.issued_by,
            valid_until: summary.valid_until,
            cookies,
            error: error.into(),
        };
    // an IPv6 address comes in brackets in a URL, not in a connection
    let host = url
        .host_str()
        .map(|h| h.trim_start_matches('[').trim_end_matches(']').to_owned());
    let (Some(host), "https") = (host, url.scheme()) else {
        post(answer(
            "",
            Default::default(),
            "This page isn't sent over a secure connection.",
        ));
        return;
    };
    let port = url.port_or_known_default().unwrap_or(443);
    if RUNNING.swap(true, Ordering::AcqRel) {
        post(answer(
            "",
            Default::default(),
            "Still checking the last site. Try again in a moment.",
        ));
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("neoweb-siteinfo".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let e = match probe(&host, port) {
                Ok((tls, der)) => match neoweb_core::safety::cert_summary(&der) {
                    Some(c) => answer(tls, c, ""),
                    None => answer(tls, Default::default(), "The certificate couldn't be read."),
                },
                Err(e) => {
                    println!("neowebe: site info {host}: {e}");
                    answer(
                        "",
                        Default::default(),
                        "The site's connection couldn't be checked.",
                    )
                }
            };
            RUNNING.store(false, Ordering::Release);
            post(e);
        });
    if let Err(e) = spawned {
        RUNNING.store(false, Ordering::Release);
        println!("neowebe: site info thread: {e}");
    }
}

/// A TLS handshake with webpki's roots (as Servo's): the protocol version and the leaf certificate.
fn probe(host: &str, port: u16) -> Result<(&'static str, Vec<u8>), String> {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_owned()).map_err(|e| e.to_string())?;
    let mut conn = ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    // P.I.P.S. sockets take no timeouts (ENOPROTOOPT): a dead site ends with the connect's own error
    let mut sock = TcpStream::connect((host, port)).map_err(|e| e.to_string())?;
    while conn.is_handshaking() {
        conn.complete_io(&mut sock).map_err(|e| e.to_string())?;
    }
    let tls = match conn.protocol_version() {
        Some(ProtocolVersion::TLSv1_3) => "TLS 1.3",
        Some(ProtocolVersion::TLSv1_2) => "TLS 1.2",
        _ => "TLS",
    };
    let der = conn
        .peer_certificates()
        .and_then(|c| c.first())
        .map(|c| c.to_vec())
        .ok_or("no certificate")?;
    conn.send_close_notify();
    let _ = conn.write_tls(&mut sock);
    Ok((tls, der))
}
