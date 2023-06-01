#[derive(Debug)]
pub enum BlameDiffError {
    FindObject(gix::odb::find::existing::Error<gix::odb::store::find::Error>),
    StrUtf8Error(std::str::Utf8Error),
    ParseSingle(gix::revision::spec::parse::single::Error),
}

impl std::fmt::Display for BlameDiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
make_error![
    gix::odb::find::existing::Error<gix::odb::store::find::Error>,
    FindObject
];
make_error![std::str::Utf8Error, StrUtf8Error];
make_error![gix::revision::spec::parse::single::Error, ParseSingle];
