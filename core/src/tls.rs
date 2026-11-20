//! HTTPS trust for every request the core makes: the operating system's certificate store on
//! Apple platforms, Windows and Linux (so a CA an organisation installs for TLS inspection is
//! trusted, as browsers do), Mozilla's roots (webpki) on Android.

pub(crate) fn config() -> ureq::tls::TlsConfig {
    #[cfg(not(target_os = "android"))]
    {
        ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build()
    }
    #[cfg(target_os = "android")]
    {
        ureq::tls::TlsConfig::default()
    }
}
