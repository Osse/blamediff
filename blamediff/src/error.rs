#[derive(Debug)]
pub enum BlameDiffError {
    BadArgs,
    Decode(gix::hash::decode::Error),
    DiscoverError(gix::discover::Error),
    PeelError(gix::object::peel::to_kind::Error),
    FindObject(gix::odb::find::existing::Error<gix::odb::store::find::Error>),
    DiffGeneration(gix::diff::tree::changes::Error),
    Io(std::io::Error),
    SystemTime(std::time::SystemTimeError),
    Parse(gix::revision::spec::parse::Error),
    ParseSingle(gix::revision::spec::parse::single::Error),
    ObtainTree(gix::object::commit::Error),
    StringUtf8Error(std::string::FromUtf8Error),
    StrUtf8Error(std::str::Utf8Error),
    WalkError(gix::revision::walk::Error),
    BlameError(gix_blame::error::Error),
}

impl std::fmt::Display for BlameDiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlameDiffError::BadArgs => write!(f, "Bad args"),
            _ => write!(f, "other badness"),
        }
    }
}

impl std::error::Error for BlameDiffError {}

macro_rules! make_error {
    ($e:ty, $b:ident) => {
        impl From<$e> for BlameDiffError {
            fn from(e: $e) -> Self {
                BlameDiffError::$b(e)
            }
        }
    };
}
make_error![gix::object::commit::Error, ObtainTree];
make_error![gix::revision::spec::parse::single::Error, ParseSingle];
make_error![
    gix::odb::find::existing::Error<gix::odb::store::find::Error>,
    FindObject
];
make_error![gix::hash::decode::Error, Decode];
make_error![gix::revision::spec::parse::Error, Parse];
make_error![gix::discover::Error, DiscoverError];
make_error![gix::diff::tree::changes::Error, DiffGeneration];
make_error![gix::object::peel::to_kind::Error, PeelError];
make_error![std::io::Error, Io];
make_error![std::time::SystemTimeError, SystemTime];
make_error![std::str::Utf8Error, StrUtf8Error];
make_error![std::string::FromUtf8Error, StringUtf8Error];
make_error![gix::revision::walk::Error, WalkError];
make_error![gix_blame::error::Error, BlameError];
