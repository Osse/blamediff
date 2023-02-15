#[derive(Debug)]
pub enum BlameDiffError {
    BadArgs,
    Decode(gix::hash::decode::Error),
    DiscoverError(gix::discover::Error),
    PeelError(gix::object::peel::to_kind::Error),
    FindObject(gix::odb::store::find::Error),
    DiffGeneration(gix::diff::tree::changes::Error),
    Io(std::io::Error),
    SystemTime(std::time::SystemTimeError),
    Parse(gix::revision::spec::parse::Error),
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
    ($e:ty, $b:expr) => {
        impl From<$e> for BlameDiffError {
            fn from(e: $e) -> Self {
                $b(e)
            }
        }
    };
}

make_error![gix::hash::decode::Error, BlameDiffError::Decode];
make_error![gix::revision::spec::parse::Error, BlameDiffError::Parse];
make_error![gix::discover::Error, BlameDiffError::DiscoverError];
make_error![
    gix::diff::tree::changes::Error,
    BlameDiffError::DiffGeneration
];
make_error![gix::object::peel::to_kind::Error, BlameDiffError::PeelError];
make_error![gix::odb::store::find::Error, BlameDiffError::FindObject];
make_error![std::io::Error, BlameDiffError::Io];
make_error![std::time::SystemTimeError, BlameDiffError::SystemTime];
