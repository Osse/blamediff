/// The error produced if a complete blame cannot be produced.
#[derive(Debug)]
pub enum Error {
    Generation,
    NotFound(std::io::Error),
    PeelError(gix::object::peel::to_kind::Error),
    FindObject(gix::odb::find::existing::Error<gix::odb::store::find::Error>),
    SystemTime(std::time::SystemTimeError),
    ParseSingle(gix::revision::spec::parse::single::Error),
    StrUtf8Error(std::str::Utf8Error),
    WalkError(gix::revision::walk::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Generation => write!(f, "Internal generation error"),
            Error::NotFound(e) => write!(f, "Path not found in tree: {}", e),
            Error::PeelError(e) => write!(f, "Error finding blob in object: {}", e),
            Error::FindObject(e) => write!(f, "Error finding object: {}", e),
            Error::ParseSingle(e) => write!(f, "Error parsing revision: {}", e),
            Error::StrUtf8Error(e) => write!(f, "Error converting data to string: {}", e),
            Error::WalkError(e) => write!(f, "Error walking the history: {}", e),
            Error::SystemTime(e) => write!(f, "Error finding timestamp: {}", e),
        }
    }
}

impl std::error::Error for Error {}

macro_rules! make_error {
    ($e:ty, $b:ident) => {
        impl From<$e> for Error {
            fn from(e: $e) -> Self {
                Error::$b(e)
            }
        }
    };
}
make_error![std::time::SystemTimeError, SystemTime];
make_error![gix::revision::spec::parse::single::Error, ParseSingle];
make_error![
    gix::odb::find::existing::Error<gix::odb::store::find::Error>,
    FindObject
];
make_error![gix::object::peel::to_kind::Error, PeelError];
make_error![std::str::Utf8Error, StrUtf8Error];
make_error![gix::revision::walk::Error, WalkError];
make_error![std::io::Error, NotFound];
