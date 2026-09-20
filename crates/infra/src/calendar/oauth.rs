use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::CalendarError;

/// Per-authorization OAuth state and PKCE material.
///
/// Keep this value until the redirect is handled, verify `state` with
/// [`Self::matches_state`], then pass [`Self::code_verifier`] to token exchange.
#[derive(Clone, PartialEq, Eq)]
pub struct OAuthPkce {
    state: String,
    code_verifier: String,
    code_challenge: String,
}

impl fmt::Debug for OAuthPkce {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OAuthPkce")
            .field("state", &self.state)
            .field("code_verifier", &"[REDACTED]")
            .field("code_challenge", &self.code_challenge)
            .finish()
    }
}

impl OAuthPkce {
    /// Generates about 244 random bits each for the CSRF state and PKCE verifier
    /// from two independent UUIDv4 values (well above RFC 7636's recommendation).
    #[must_use]
    pub fn generate() -> Self {
        let state = random_hex_256();
        let code_verifier = random_hex_256();
        Self::from_valid_parts(state, code_verifier)
    }

    /// Restores material persisted by a caller while an OAuth redirect is in flight.
    pub fn from_parts(
        state: impl Into<String>,
        code_verifier: impl Into<String>,
    ) -> Result<Self, CalendarError> {
        let state = state.into();
        let code_verifier = code_verifier.into();
        if state.is_empty() {
            return Err(CalendarError::InvalidResponse(
                "OAuth state must not be empty".into(),
            ));
        }
        if !(43..=128).contains(&code_verifier.len())
            || !code_verifier.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
            })
        {
            return Err(CalendarError::InvalidResponse(
                "PKCE verifier must be 43-128 RFC 7636 unreserved characters".into(),
            ));
        }
        Ok(Self::from_valid_parts(state, code_verifier))
    }

    fn from_valid_parts(state: String, code_verifier: String) -> Self {
        let digest = Sha256::digest(code_verifier.as_bytes());
        let code_challenge = URL_SAFE_NO_PAD.encode(digest);
        Self {
            state,
            code_verifier,
            code_challenge,
        }
    }

    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    #[must_use]
    pub fn code_verifier(&self) -> &str {
        &self.code_verifier
    }

    #[must_use]
    pub fn code_challenge(&self) -> &str {
        &self.code_challenge
    }

    /// Compares callback state without an early exit on differing bytes.
    #[must_use]
    pub fn matches_state(&self, candidate: &str) -> bool {
        if self.state.len() != candidate.len() {
            return false;
        }
        self.state
            .bytes()
            .zip(candidate.bytes())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
    }
}

fn random_hex_256() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rfc_7636_s256_example() {
        let pkce =
            OAuthPkce::from_parts("state", "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").unwrap();
        assert_eq!(
            pkce.code_challenge(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(pkce.matches_state("state"));
        assert!(!pkce.matches_state("other"));
        assert!(!format!("{pkce:?}").contains(pkce.code_verifier()));
    }

    #[test]
    fn generated_material_has_valid_lengths() {
        let pkce = OAuthPkce::generate();
        assert_eq!(pkce.state().len(), 64);
        assert_eq!(pkce.code_verifier().len(), 64);
        assert_eq!(pkce.code_challenge().len(), 43);
    }
}
