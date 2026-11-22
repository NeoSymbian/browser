// SPDX-License-Identifier: Apache-2.0
//! The safety pages' logic (UI5): what a certificate says (`br2-cert-error`, `br2-siteinfo`) and why a page didn't
//! load (`br2-offline`).

use alloc::string::String;
use alloc::vec::Vec;

/// What NEOWEB shows of a certificate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CertSummary {
    /// The subject's common name (else its organisation).
    pub issued_to: String,
    /// The issuer's common name (else its organisation).
    pub issued_by: String,
    /// The start of the validity, `YYYY-MM-DD`.
    pub valid_from: String,
    /// The end of the validity, `YYYY-MM-DD`.
    pub valid_until: String,
}

/// Read a DER certificate's subject, issuer and end of validity; `None` if it isn't one.
pub fn cert_summary(der: &[u8]) -> Option<CertSummary> {
    let (_, cert, _) = tlv(der)?; // Certificate
    let (_, mut tbs, _) = tlv(cert)?; // TBSCertificate
    let (tag, _, rest) = tlv(tbs)?;
    if tag == 0xa0 {
        tbs = rest; // [0] version
    }
    let (_, _, rest) = tlv(tbs)?; // serialNumber
    let (_, _, rest) = tlv(rest)?; // signature
    let (_, issuer, rest) = tlv(rest)?;
    let (_, validity, rest) = tlv(rest)?;
    let (_, subject, _) = tlv(rest)?;
    let (from_tag, not_before, after) = tlv(validity)?;
    let (until_tag, not_after, _) = tlv(after)?;
    Some(CertSummary {
        issued_to: name(subject)?,
        issued_by: name(issuer)?,
        valid_from: date(from_tag, not_before)?,
        valid_until: date(until_tag, not_after)?,
    })
}

/// One DER tag-length-value: (tag, value, the bytes after it). Short tags only (all X.509 needs).
fn tlv(b: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, b) = b.split_first()?;
    let (&first, b) = b.split_first()?;
    let (len, b) = if first < 0x80 {
        (first as usize, b)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || b.len() < n {
            return None;
        }
        (
            b[..n].iter().fold(0usize, |l, &x| (l << 8) | x as usize),
            &b[n..],
        )
    };
    (b.len() >= len).then(|| (tag, &b[..len], &b[len..]))
}

/// A Name's common name (2.5.4.3), else its organisation (2.5.4.10).
fn name(mut rdns: &[u8]) -> Option<String> {
    let (mut cn, mut o) = (None, None);
    while !rdns.is_empty() {
        let (_, set, rest) = tlv(rdns)?;
        rdns = rest;
        let (_, atv, _) = tlv(set)?;
        let (_, oid, rest) = tlv(atv)?;
        let (_, value, _) = tlv(rest)?;
        let text = String::from_utf8_lossy(value).into_owned();
        match oid {
            [0x55, 0x04, 0x03] => cn = Some(text),
            [0x55, 0x04, 0x0a] => o = Some(text),
            _ => {}
        }
    }
    cn.or(o).or_else(|| Some(String::new()))
}

/// UTCTime (`YYMMDD…`, 1950–2049) or GeneralizedTime (`YYYYMMDD…`) as `YYYY-MM-DD`.
fn date(tag: u8, t: &[u8]) -> Option<String> {
    let t = core::str::from_utf8(t).ok()?;
    let (year, rest) = match tag {
        0x17 => {
            let yy: u32 = t.get(..2)?.parse().ok()?;
            (if yy < 50 { 2000 + yy } else { 1900 + yy }, t.get(2..)?)
        }
        0x18 => (t.get(..4)?.parse().ok()?, t.get(4..)?),
        _ => return None,
    };
    let (m, d) = (rest.get(..2)?, rest.get(2..4)?);
    (m.bytes().chain(d.bytes()).all(|c| c.is_ascii_digit()))
        .then(|| alloc::format!("{year}-{m}-{d}"))
}

/// Standard base64, with or without padding (the certificate Servo puts in its error page).
pub fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// The date (`YYYY-MM-DD`, UTC) of a Unix time in seconds.
pub fn date_of(unix_secs: i64) -> String {
    // days to a civil date (Howard Hinnant's algorithm)
    let z = unix_secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    alloc::format!("{y:04}-{m:02}-{d:02}")
}

/// Servo often says only "client error (Connect)" for a bad certificate: the certificate's own dates against
/// `today` (`YYYY-MM-DD`) name an expired or not-yet-valid one for [`cert_cause`].
pub fn cert_reason(reason: &str, cert: &CertSummary, today: &str) -> String {
    if !cert.valid_until.is_empty() && cert.valid_until.as_str() < today {
        alloc::format!("Expired ({reason})")
    } else if !cert.valid_from.is_empty() && cert.valid_from.as_str() > today {
        alloc::format!("not valid yet ({reason})")
    } else {
        reason.into()
    }
}

/// `YYYY-MM-DD` as "12 Apr 2015" (the boards' dates); anything else as it is.
pub fn pretty_date(d: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = d.splitn(3, '-');
    let (Some(y), Some(m), Some(day)) = (parts.next(), parts.next(), parts.next()) else {
        return d.into();
    };
    match (m.parse::<usize>(), day.parse::<u32>()) {
        (Ok(m @ 1..=12), Ok(day)) => alloc::format!("{day} {} {y}", MONTHS[m - 1]),
        _ => d.into(),
    }
}

/// Why a certificate failed, for `br2-cert-error`: the sentence after the site's name, and the error code.
pub fn cert_cause(reason: &str, cert: &CertSummary) -> (String, &'static str) {
    let r = reason.to_ascii_lowercase();
    if r.contains("expired") {
        let when = pretty_date(&cert.valid_until);
        let s = if when.is_empty() {
            "sent a certificate that has expired.".into()
        } else {
            alloc::format!("sent a certificate that expired on {when}.")
        };
        (s, "CERT_DATE_INVALID")
    } else if r.contains("notvalidyet") || r.contains("not valid yet") {
        (
            "sent a certificate that isn't valid yet.".into(),
            "CERT_DATE_INVALID",
        )
    } else if r.contains("unknownissuer") || r.contains("unknown issuer") || r.contains("self") {
        (
            "sent a certificate from an issuer this phone doesn't trust.".into(),
            "CERT_AUTHORITY_INVALID",
        )
    } else if r.contains("notvalidforname") || r.contains("name") {
        (
            alloc::format!("sent a certificate for another site ({}).", cert.issued_to),
            "CERT_COMMON_NAME_INVALID",
        )
    } else if r.contains("revoked") {
        (
            "sent a certificate that has been revoked.".into(),
            "CERT_REVOKED",
        )
    } else {
        (
            "sent a certificate that couldn't be checked.".into(),
            "CERT_INVALID",
        )
    }
}

/// Why a page didn't load, for `br2-offline`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetCause {
    /// the page's title
    pub title: &'static str,
    /// after "<site> couldn't be reached: "
    pub cause: &'static str,
    /// the short line before the code
    pub short: &'static str,
    pub code: &'static str,
}

/// The cause of a network error from the engine's reason (Servo's error name or hyper's message).
pub fn net_cause(reason: &str) -> NetCause {
    let r = reason.to_ascii_lowercase();
    let c = |title, cause, short, code| NetCause {
        title,
        cause,
        short,
        code,
    };
    const OFFLINE: &str = "No internet connection";
    const UNREACHABLE: &str = "This site can't be reached";
    if r.contains("dns") || r.contains("lookup") || r.contains("resolve") {
        c(
            OFFLINE,
            "the address could not be looked up",
            "DNS lookup failed",
            "NAME_NOT_RESOLVED",
        )
    } else if r.contains("unreachable")
        || r.contains("network is down")
        || r.contains("connectionfailure")
    {
        c(
            OFFLINE,
            "the phone isn't connected",
            "No network",
            "INTERNET_DISCONNECTED",
        )
    } else if r.contains("refused") {
        c(
            UNREACHABLE,
            "it refused the connection",
            "Connection refused",
            "CONNECTION_REFUSED",
        )
    } else if r.contains("timed out") || r.contains("timeout") {
        c(
            UNREACHABLE,
            "it took too long to answer",
            "Timed out",
            "TIMED_OUT",
        )
    } else if r.contains("redirect") {
        c(
            "This page isn't working",
            "it redirected too many times",
            "Too many redirects",
            "TOO_MANY_REDIRECTS",
        )
    } else {
        c(
            UNREACHABLE,
            "the page couldn't be loaded",
            "Load failed",
            "FAILED",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_certificate() {
        // openssl req -x509 (P-256): /O=Neo Test/CN=test.example, notAfter Dec 26 2027 (UTCTime)
        let c = cert_summary(include_bytes!("testdata/short.der")).unwrap();
        assert_eq!(c.issued_to, "test.example");
        assert_eq!(c.issued_by, "test.example");
        assert_eq!(c.valid_until, "2027-12-26");
        assert_eq!(c.valid_from, "2026-11-21");
        // notAfter 2054: GeneralizedTime
        let c = cert_summary(include_bytes!("testdata/long.der")).unwrap();
        assert_eq!(
            (c.issued_to.as_str(), c.valid_until.as_str()),
            ("long.example", "2054-04-08")
        );
    }

    #[test]
    fn rejects_what_isnt_a_certificate() {
        assert_eq!(cert_summary(b""), None);
        assert_eq!(cert_summary(&[0x30, 0x82, 0xff, 0xff, 0x30]), None);
        let der = include_bytes!("testdata/short.der");
        assert_eq!(cert_summary(&der[..100]), None);
    }

    #[test]
    fn decodes_base64() {
        assert_eq!(base64("TmVv").unwrap(), b"Neo");
        assert_eq!(base64("TmVvUw").unwrap(), b"NeoS");
        assert_eq!(base64("TmVvUw==").unwrap(), b"NeoS");
        assert_eq!(base64("Tm V"), None);
        // a whole certificate, the way Servo writes it (STANDARD_NO_PAD)
        let der = include_bytes!("testdata/short.der");
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut text = String::new();
        for c in der.chunks(3) {
            let n = c.iter().fold(0u32, |a, &b| (a << 8) | b as u32) << (8 * (3 - c.len()));
            for i in 0..=c.len() {
                text.push(A[(n >> (18 - 6 * i)) as usize & 63] as char);
            }
        }
        assert_eq!(base64(&text).unwrap(), der);
    }

    #[test]
    fn names_the_cause() {
        assert_eq!(
            net_cause("dns error: failed to lookup address information").code,
            "NAME_NOT_RESOLVED"
        );
        assert_eq!(
            net_cause("Connection refused (os error 111)").code,
            "CONNECTION_REFUSED"
        );
        assert_eq!(
            net_cause("ConnectionFailure").title,
            "No internet connection"
        );
        assert_eq!(net_cause("TooManyRedirects").code, "TOO_MANY_REDIRECTS");
        assert_eq!(net_cause("something else").code, "FAILED");
        let cert = CertSummary {
            issued_to: "*.badssl.com".into(),
            valid_until: "2015-04-12".into(),
            ..Default::default()
        };
        assert_eq!(
            cert_cause("invalid peer certificate: Expired", &cert),
            (
                "sent a certificate that expired on 12 Apr 2015.".into(),
                "CERT_DATE_INVALID"
            )
        );
        assert_eq!(
            cert_cause("invalid peer certificate: UnknownIssuer", &cert).1,
            "CERT_AUTHORITY_INVALID"
        );
        assert_eq!(
            cert_cause("invalid peer certificate: NotValidForName", &cert).0,
            "sent a certificate for another site (*.badssl.com)."
        );
        assert_eq!(pretty_date("2027-01-11"), "11 Jan 2027");
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1_795_286_149), "2026-11-21");
        assert_eq!(date_of(951_782_400), "2000-02-29");
        let expired = cert_reason("client error (Connect)", &cert, "2026-11-22");
        assert_eq!(cert_cause(&expired, &cert).1, "CERT_DATE_INVALID");
        assert_eq!(
            cert_reason("client error (Connect)", &cert, "2015-04-01"),
            "client error (Connect)"
        );
        assert_eq!(pretty_date(""), "");
    }
}
