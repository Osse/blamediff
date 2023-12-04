/// The error produced if a complete blame cannot be produced.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("lol")]
    Generation,
    #[error("lol")]
    InvalidRange,
    #[error("lol")]
    NotFound(#[from] std::io::Error),
    #[error("lol")]
    PeelError(#[from] gix::object::peel::to_kind::Error),
    #[error("lol")]
    FindObject(#[from] gix::object::find::existing::Error),
    #[error("lol")]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error("lol")]
    ParseSingle(#[from] gix::revision::spec::parse::single::Error),
    #[error("lol")]
    Parse(#[from] gix::revision::spec::parse::Error),
    #[error("lol")]
    StrUtf8Error(#[from] std::str::Utf8Error),
    #[error("lol")]
    WalkError(#[from] gix::revision::walk::Error),
    #[error("lol")]
    TopoError(#[from] topo::Error),
}
