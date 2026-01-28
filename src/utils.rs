use base64::{Engine, prelude::BASE64_STANDARD};
use boringtun::x25519::{PublicKey, StaticSecret};

use crate::errors::{ConetResult, Error};

pub fn base64_to_private_key(s: String) -> ConetResult<StaticSecret> {
    let key_result: Result<[u8; 32], Vec<u8>> = BASE64_STANDARD.decode(s)?.try_into();
    let key = key_result.map_err(|_| Error::Err("cannot parse private_key".to_string()))?;

    Ok(StaticSecret::from(key))
}

pub fn base64_to_public_key(s: String) -> ConetResult<PublicKey> {
    let key_result: Result<[u8; 32], Vec<u8>> = BASE64_STANDARD.decode(s)?.try_into();
    let key = key_result.map_err(|_| Error::Err("cannot parse public_key".to_string()))?;

    Ok(PublicKey::from(key))
}
