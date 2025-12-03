use base64::DecodeError;

pub type ConetResult<T> = Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Err(String),

    #[error(transparent)]
    Base64(#[from] DecodeError),

    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    Errno(#[from] nix::Error),

    #[error(transparent)]
    ReceiverErr(#[from] async_channel::RecvError),
}
