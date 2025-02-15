// This file is dual licensed under the terms of the Apache License, Version
// 2.0, and the BSD License. See the LICENSE file in the root of this repository
// for complete details.

#![deny(rust_2018_idioms, clippy::undocumented_unsafe_blocks)]
#![allow(unknown_lints, non_local_definitions, clippy::result_large_err)]

#[cfg(CRYPTOGRAPHY_OPENSSL_300_OR_GREATER)]
use crate::error::CryptographyResult;
#[cfg(CRYPTOGRAPHY_OPENSSL_300_OR_GREATER)]
use openssl::provider;
#[cfg(CRYPTOGRAPHY_OPENSSL_300_OR_GREATER)]
use std::env;

mod asn1;
mod backend;
mod buf;
mod error;
mod exceptions;
pub(crate) mod oid;
mod padding;
mod pkcs12;
mod pkcs7;
pub(crate) mod types;
mod x509;

#[cfg(CRYPTOGRAPHY_OPENSSL_300_OR_GREATER)]
#[pyo3::pyclass(module = "cryptography.hazmat.bindings._rust")]
struct LoadedProviders {
    legacy: Option<provider::Provider>,
    _default: provider::Provider,

    fips: Option<provider::Provider>,
}

#[pyo3::pyfunction]
fn openssl_version() -> i64 {
    openssl::version::number()
}

#[pyo3::pyfunction]
fn openssl_version_text() -> &'static str {
    openssl::version::version()
}

#[pyo3::pyfunction]
fn is_fips_enabled() -> bool {
    cryptography_openssl::fips::is_enabled()
}

#[cfg(CRYPTOGRAPHY_OPENSSL_300_OR_GREATER)]
fn _initialize_providers() -> CryptographyResult<LoadedProviders> {
    // As of OpenSSL 3.0.0 we must register a legacy cipher provider
    // to get RC2 (needed for junk asymmetric private key
    // serialization), RC4, Blowfish, IDEA, SEED, etc. These things
    // are ugly legacy, but we aren't going to get rid of them
    // any time soon.
    let load_legacy = env::var("CRYPTOGRAPHY_OPENSSL_NO_LEGACY")
        .map(|v| v.is_empty() || v == "0")
        .unwrap_or(true);
    let legacy = if load_legacy {
        let legacy_result = provider::Provider::load(None, "legacy");
        _legacy_provider_error(legacy_result.is_ok())?;
        Some(legacy_result?)
    } else {
        None
    };
    let _default = provider::Provider::load(None, "default")?;
    Ok(LoadedProviders {
        legacy,
        _default,
        fips: None,
    })
}

fn _legacy_provider_error(success: bool) -> pyo3::PyResult<()> {
    if !success {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(
            "OpenSSL 3.0's legacy provider failed to load. This is a fatal error by default, but cryptography supports running without legacy algorithms by setting the environment variable CRYPTOGRAPHY_OPENSSL_NO_LEGACY. If you did not expect this error, you have likely made a mistake with your OpenSSL configuration."
        ));
    }
    Ok(())
}

#[cfg(CRYPTOGRAPHY_OPENSSL_300_OR_GREATER)]
#[pyo3::pyfunction]
fn enable_fips(providers: &mut LoadedProviders) -> CryptographyResult<()> {
    providers.fips = Some(provider::Provider::load(None, "fips")?);
    cryptography_openssl::fips::enable()?;
    Ok(())
}

#[pyo3::pymodule]
mod _rust {
    use pyo3::types::PyModuleMethods;

    #[pymodule_export]
    use crate::asn1::asn1_mod;
    #[pymodule_export]
    use crate::exceptions::exceptions;
    #[pymodule_export]
    use crate::oid::ObjectIdentifier;
    #[pymodule_export]
    use crate::padding::{check_ansix923_padding, check_pkcs7_padding, PKCS7PaddingContext};
    #[pymodule_export]
    use crate::pkcs12::pkcs12;

    #[pyo3::pymodule]
    mod x509 {
        #[pymodule_export]
        use crate::x509::verify::{
            PolicyBuilder, PyClientVerifier, PyServerVerifier, PyStore, PyVerifiedClient,
            VerificationError,
        };

        #[pymodule_init]
        fn init(x509_mod: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
            crate::x509::certificate::add_to_module(x509_mod)?;
            crate::x509::common::add_to_module(x509_mod)?;
            crate::x509::crl::add_to_module(x509_mod)?;
            crate::x509::csr::add_to_module(x509_mod)?;
            crate::x509::sct::add_to_module(x509_mod)?;

            Ok(())
        }
    }

    #[pyo3::pymodule]
    mod ocsp {
        #[pymodule_export]
        use crate::x509::ocsp_req::{create_ocsp_request, load_der_ocsp_request, OCSPRequest};

        #[pymodule_init]
        fn init(ocsp_mod: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
            crate::x509::ocsp_resp::add_to_module(ocsp_mod)?;

            Ok(())
        }
    }

    #[pyo3::pymodule]
    mod openssl {
        use pyo3::prelude::PyModuleMethods;

        #[cfg(CRYPTOGRAPHY_OPENSSL_300_OR_GREATER)]
        #[pymodule_export]
        use super::super::enable_fips;
        #[pymodule_export]
        use super::super::{is_fips_enabled, openssl_version, openssl_version_text};
        #[pymodule_export]
        use crate::error::{capture_error_stack, raise_openssl_error, OpenSSLError};

        #[pymodule_init]
        fn init(openssl_mod: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
            openssl_mod.add(
                "CRYPTOGRAPHY_OPENSSL_300_OR_GREATER",
                cfg!(CRYPTOGRAPHY_OPENSSL_300_OR_GREATER),
            )?;
            openssl_mod.add(
                "CRYPTOGRAPHY_OPENSSL_320_OR_GREATER",
                cfg!(CRYPTOGRAPHY_OPENSSL_320_OR_GREATER),
            )?;

            openssl_mod.add("CRYPTOGRAPHY_IS_LIBRESSL", cfg!(CRYPTOGRAPHY_IS_LIBRESSL))?;
            openssl_mod.add("CRYPTOGRAPHY_IS_BORINGSSL", cfg!(CRYPTOGRAPHY_IS_BORINGSSL))?;

            cfg_if::cfg_if! {
                if #[cfg(CRYPTOGRAPHY_OPENSSL_300_OR_GREATER)] {
                    let providers = super::super::_initialize_providers()?;
                    if providers.legacy.is_some() {
                        openssl_mod.add("_legacy_provider_loaded", true)?;
                    } else {
                        openssl_mod.add("_legacy_provider_loaded", false)?;
                    }
                    openssl_mod.add("_providers", providers)?;
                } else {
                    // default value for non-openssl 3+
                    openssl_mod.add("_legacy_provider_loaded", false)?;
                }
            }

            crate::backend::add_to_module(openssl_mod)?;

            Ok(())
        }
    }

    #[pymodule_init]
    fn init(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
        m.add_submodule(&crate::pkcs7::create_submodule(m.py())?)?;
        m.add_submodule(&cryptography_cffi::create_module(m.py())?)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::_legacy_provider_error;

    #[test]
    fn test_legacy_provider_error() {
        assert!(_legacy_provider_error(true).is_ok());
        assert!(_legacy_provider_error(false).is_err());
    }
}
