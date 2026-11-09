//! Provider secrets at rest, per 00004 D5: AES-256-GCM under `CREDENTIALS_KEY`.

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

pub struct Credentials {
    key: LessSafeKey,
    random: SystemRandom,
}

#[derive(Debug)]
pub struct Unreadable;

impl Credentials {
    /// Reads the key as 64 hex characters, the 32 bytes `openssl rand -hex 32`
    /// writes.
    pub fn from_hex(hex: &str) -> Result<Self, String> {
        const WRONG: &str =
            "CREDENTIALS_KEY must be 64 hex characters. Generate it with `openssl rand -hex 32`.";
        // `from_str_radix` alone would also take a sign, such as "+f".
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(WRONG.into());
        }
        let bytes: Vec<u8> = (0..32)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("checked as hex"))
            .collect();
        let key = UnboundKey::new(&AES_256_GCM, &bytes).expect("the key is 32 bytes");
        Ok(Self {
            key: LessSafeKey::new(key),
            random: SystemRandom::new(),
        })
    }

    /// Encrypts `secret` for the row `owner` names. `open` refuses it under any
    /// other owner, so a ciphertext copied to another row does not decrypt.
    pub fn seal(&self, owner: &str, secret: &str) -> Vec<u8> {
        let mut nonce = [0; NONCE_LEN];
        self.random
            .fill(&mut nonce)
            .expect("the system's random source works");
        let mut sealed = secret.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(owner),
                &mut sealed,
            )
            .expect("a secret is far below AES-GCM's length limit");
        [nonce.as_slice(), &sealed].concat()
    }

    pub fn open(&self, owner: &str, sealed: &[u8]) -> Result<String, Unreadable> {
        if sealed.len() < NONCE_LEN {
            return Err(Unreadable);
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| Unreadable)?;
        let mut buffer = ciphertext.to_vec();
        let plaintext = self
            .key
            .open_in_place(nonce, Aad::from(owner), &mut buffer)
            .map_err(|_| Unreadable)?;
        String::from_utf8(plaintext.to_vec()).map_err(|_| Unreadable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn a_sealed_secret_opens_under_the_same_key_and_owner() {
        let sealed = Credentials::from_hex(KEY).unwrap().seal("chn_a", "token");
        assert!(!sealed.windows(5).any(|window| window == b"token"));
        let reopened = Credentials::from_hex(KEY).unwrap().open("chn_a", &sealed);
        assert_eq!(reopened.unwrap(), "token");
    }

    #[test]
    fn sealing_twice_gives_different_ciphertexts() {
        let credentials = Credentials::from_hex(KEY).unwrap();
        assert_ne!(
            credentials.seal("chn_a", "token"),
            credentials.seal("chn_a", "token")
        );
    }

    #[test]
    fn a_secret_does_not_open_for_another_owner_or_key() {
        let sealed = Credentials::from_hex(KEY).unwrap().seal("chn_a", "token");
        let credentials = Credentials::from_hex(KEY).unwrap();
        assert!(credentials.open("chn_b", &sealed).is_err());
        let other = Credentials::from_hex(&KEY.replace("1f", "20")).unwrap();
        assert!(other.open("chn_a", &sealed).is_err());
        assert!(credentials.open("chn_a", &sealed[..8]).is_err());
    }

    #[test]
    fn the_key_must_be_64_hex_characters() {
        for key in [
            "",
            &KEY[..62],
            &format!("{KEY}00"),
            &KEY.replace('0', "g"),
            &KEY.replacen("00", "+0", 1),
            &KEY.replacen("00", "é", 1),
        ] {
            assert!(Credentials::from_hex(key).is_err(), "{key:?}");
        }
    }
}
