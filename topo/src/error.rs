use thiserror;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Calculated indegree missing")]
    MissingIndegree,
    #[error("Internal state not found")]
    MissingState,
    #[error("Commit not found in commit graph or storage")]
    CommitNotFound,
    #[error("Error initializing graph: {0}")]
    CommitGraphInit(#[from] gix_commitgraph::init::Error),
    #[error("Error doing file stuff: {0}")]
    CommitGraphFile(#[from] gix_commitgraph::file::commit::Error),
    #[error("Error decoding stuff: {0}")]
    ObjectDecode(#[from] gix_object::decode::Error),
}
