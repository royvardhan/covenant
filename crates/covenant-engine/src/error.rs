use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        message: String,
    },
    DuplicateId(String),
    EmptyPaths(String),
    BadGlob {
        id: String,
        glob: String,
        message: String,
    },
    Git(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Parse { path, message } => write!(f, "{}: {message}", path.display()),
            Error::DuplicateId(id) => write!(f, "promise id `{id}` is declared more than once"),
            Error::EmptyPaths(id) => write!(f, "promise `{id}` has no paths"),
            Error::BadGlob { id, glob, message } => {
                write!(f, "promise `{id}`: invalid path glob `{glob}`: {message}")
            }
            Error::Git(message) => write!(f, "git: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
