//! The bridge's per-process token and the proofs built on it.
//!
//! Ottid mints a fresh random token each time it starts and keeps it in a
//! file only the user can read ([`super::endpoint`]). The token itself never
//! crosses the wire. Instead, each end sends a fresh nonce, and every message
//! after the hellos carries an HMAC-SHA256 under the token over both nonces
//! and the message:
//!
//! - the listener's challenge proves it holds the token before the client
//!   says what Claude Code wants to run;
//! - the client's request proves it holds the token before a card is shown;
//! - the answer proves it came from the listener that saw this request, so
//!   nothing else on the endpoint can say "allow".
//!
//! Every MAC is checked in constant time (`hmac`'s `verify_slice`). A label
//! per message keeps a MAC for one kind from passing for another.

use std::fmt;
use std::io;

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Bytes in the token.
pub const TOKEN_LEN: usize = 32;
/// Bytes in a nonce.
pub const NONCE_LEN: usize = 32;
/// Bytes in a MAC.
pub const MAC_LEN: usize = 32;

/// What a MAC is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The listener's proof, in its challenge.
    Challenge,
    /// The client's request.
    Ask,
    /// The listener's answer.
    Answer,
}

impl Purpose {
    fn label(self) -> &'static [u8] {
        match self {
            Purpose::Challenge => b"ottid-agent-bridge/1/challenge",
            Purpose::Ask => b"ottid-agent-bridge/1/ask",
            Purpose::Answer => b"ottid-agent-bridge/1/answer",
        }
    }
}

/// The per-process secret. Its `Debug` doesn't show it, so it can't reach a
/// log by accident.
#[derive(Clone)]
pub struct Token([u8; TOKEN_LEN]);

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

impl Token {
    /// A new token from the OS's random source.
    pub fn generate() -> io::Result<Self> {
        Ok(Self(random()?))
    }

    /// Read a token from its hex form, as the bridge file holds it.
    pub fn from_hex(text: &str) -> Option<Self> {
        from_hex(text).map(Self)
    }

    /// The hex form, for the bridge file only.
    pub(crate) fn to_hex(&self) -> String {
        to_hex(&self.0)
    }

    #[cfg(test)]
    pub(crate) fn from_bytes(bytes: [u8; TOKEN_LEN]) -> Self {
        Self(bytes)
    }
}

/// A one-time value each end sends in its hello.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nonce([u8; NONCE_LEN]);

impl Nonce {
    /// A new nonce from the OS's random source.
    pub fn generate() -> io::Result<Self> {
        Ok(Self(random()?))
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        from_hex(text).map(Self)
    }

    pub fn to_hex(&self) -> String {
        to_hex(&self.0)
    }

    #[cfg(test)]
    pub(crate) fn from_bytes(bytes: [u8; NONCE_LEN]) -> Self {
        Self(bytes)
    }
}

/// One exchange: the token and the two nonces every MAC covers.
pub struct Session<'a> {
    token: &'a Token,
    client: Nonce,
    server: Nonce,
}

impl<'a> Session<'a> {
    pub fn new(token: &'a Token, client: Nonce, server: Nonce) -> Self {
        Self {
            token,
            client,
            server,
        }
    }

    fn hmac(&self, purpose: Purpose, body: &[u8]) -> HmacSha256 {
        // HMAC takes a key of any length; this can't fail.
        let mut mac = HmacSha256::new_from_slice(&self.token.0).expect("HMAC takes any key length");
        let label = purpose.label();
        mac.update(&[label.len() as u8]);
        mac.update(label);
        mac.update(&self.client.0);
        mac.update(&self.server.0);
        mac.update(body);
        mac
    }

    /// The MAC for `body`.
    pub fn tag(&self, purpose: Purpose, body: &[u8]) -> [u8; MAC_LEN] {
        self.hmac(purpose, body).finalize().into_bytes().into()
    }

    /// Whether `tag` is the MAC for `body`, compared in constant time.
    pub fn verify(&self, purpose: Purpose, body: &[u8], tag: &[u8]) -> bool {
        self.hmac(purpose, body).verify_slice(tag).is_ok()
    }

    /// The listener's proof that it holds the token, for its challenge.
    pub fn proof(&self) -> String {
        to_hex(&self.tag(Purpose::Challenge, &[]))
    }

    /// Whether the challenge's proof is right.
    pub fn check_proof(&self, proof: &str) -> bool {
        match from_hex::<MAC_LEN>(proof) {
            Some(tag) => self.verify(Purpose::Challenge, &[], &tag),
            None => false,
        }
    }

    /// `body` with its MAC in front, as a request or answer frame carries it.
    pub fn seal(&self, purpose: Purpose, body: &[u8]) -> Vec<u8> {
        let mut sealed = Vec::with_capacity(MAC_LEN + body.len());
        sealed.extend_from_slice(&self.tag(purpose, body));
        sealed.extend_from_slice(body);
        sealed
    }

    /// The body of a sealed frame, if its MAC is right.
    pub fn open<'b>(&self, purpose: Purpose, sealed: &'b [u8]) -> Option<&'b [u8]> {
        if sealed.len() < MAC_LEN {
            return None;
        }
        let (tag, body) = sealed.split_at(MAC_LEN);
        self.verify(purpose, body, tag).then_some(body)
    }
}

fn random<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes)
        .map_err(|err| io::Error::other(format!("no random source: {err}")))?;
    Ok(bytes)
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 0x0F)] as char);
    }
    text
}

fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.as_bytes();
    if text.len() != N * 2 {
        return None;
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut bytes = [0u8; N];
    for (i, pair) in text.chunks_exact(2).enumerate() {
        bytes[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(token: &Token) -> Session<'_> {
        Session::new(
            token,
            Nonce::from_bytes([1; 32]),
            Nonce::from_bytes([2; 32]),
        )
    }

    #[test]
    fn hmac_sha256_matches_rfc_4231_case_2() {
        // The primitive under every MAC here.
        let mut mac = HmacSha256::new_from_slice(b"Jefe").unwrap();
        mac.update(b"what do ya want for nothing?");
        assert_eq!(
            to_hex(&mac.finalize().into_bytes()),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn a_token_round_trips_through_hex_and_never_shows_in_debug() {
        let token = Token::generate().unwrap();
        let hex = token.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(Token::from_hex(&hex).unwrap().0, token.0);
        assert_eq!(format!("{token:?}"), "Token(..)");
        assert!(!format!("{token:?}").contains(&hex[..8]));
    }

    #[test]
    fn two_tokens_differ() {
        assert_ne!(Token::generate().unwrap().0, Token::generate().unwrap().0);
        assert_ne!(Nonce::generate().unwrap(), Nonce::generate().unwrap());
    }

    #[test]
    fn hex_of_the_wrong_length_or_digits_is_refused() {
        assert!(Token::from_hex("").is_none());
        assert!(Token::from_hex(&"a".repeat(63)).is_none());
        assert!(Token::from_hex(&"a".repeat(65)).is_none());
        assert!(Token::from_hex(&"g".repeat(64)).is_none());
        assert!(Token::from_hex(&format!("{}א", "a".repeat(62))).is_none());
        assert!(Token::from_hex(&"AB".repeat(32)).is_some());
    }

    #[test]
    fn the_proof_checks_only_with_the_same_token_and_nonces() {
        let token = Token::from_bytes([9; 32]);
        let proof = session(&token).proof();
        assert!(session(&token).check_proof(&proof));

        let other = Token::from_bytes([8; 32]);
        assert!(!session(&other).check_proof(&proof), "another token");

        let replay = Session::new(
            &token,
            Nonce::from_bytes([3; 32]),
            Nonce::from_bytes([2; 32]),
        );
        assert!(!replay.check_proof(&proof), "another client nonce");

        assert!(!session(&token).check_proof("not hex"));
        assert!(!session(&token).check_proof(&proof[..62]));
    }

    #[test]
    fn a_sealed_body_opens_only_unchanged_and_for_its_purpose() {
        let token = Token::from_bytes([9; 32]);
        let body = br#"{"tool":"Bash","input":{"command":"rm -rf build"}}"#;
        let sealed = session(&token).seal(Purpose::Ask, body);
        assert_eq!(session(&token).open(Purpose::Ask, &sealed), Some(&body[..]));

        // A MAC for a request doesn't pass for an answer.
        assert_eq!(session(&token).open(Purpose::Answer, &sealed), None);

        // One bit changed anywhere.
        for at in [0, MAC_LEN, sealed.len() - 1] {
            let mut tampered = sealed.clone();
            tampered[at] ^= 1;
            assert_eq!(
                session(&token).open(Purpose::Ask, &tampered),
                None,
                "byte {at}"
            );
        }

        // Too short to hold a MAC.
        assert_eq!(
            session(&token).open(Purpose::Ask, &sealed[..MAC_LEN - 1]),
            None
        );

        // Another token.
        let other = Token::from_bytes([8; 32]);
        assert_eq!(session(&other).open(Purpose::Ask, &sealed), None);
    }
}
