use std::io::{Read, Write, ErrorKind};
use std::fmt::{self, Debug};

use response::{Responder, Outcome};
use http::hyper::FreshHyperResponse;
use outcome::Outcome::*;

// TODO: Support custom chunk sizes.
/// The default size of each chunk in the streamed response.
pub const CHUNK_SIZE: usize = 4096;

pub struct Stream<T: Read>(Box<T>);

impl<T: Read> Stream<T> {
    pub fn from(reader: T) -> Stream<T> {
        Stream(Box::new(reader))
    }

    //     pub fn chunked(mut self, size: usize) -> Self {
    //         self.1 = size;
    //         self
    //     }

    //     #[inline(always)]
    //     pub fn chunk_size(&self) -> usize {
    //         self.1
    //     }
}

impl<T: Read + Debug> Debug for Stream<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Stream({:?})", self.0)
    }
}

impl<T: Read> Responder for Stream<T> {
    fn respond<'a>(&mut self, res: FreshHyperResponse<'a>) -> Outcome<'a> {
        let mut stream = match res.start() {
            Ok(s) => s,
            Err(ref err) => {
                error_!("Failed opening response stream: {:?}", err);
                return Failure(());
            }
        };

        let mut buffer = [0; CHUNK_SIZE];
        let mut complete = false;
        while !complete {
            let mut read = 0;
            while read < buffer.len() && !complete {
                match self.0.read(&mut buffer[read..]) {
                    Ok(n) if n == 0 => complete = true,
                    Ok(n) => read += n,
                    Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(ref e) => {
                        error_!("Error streaming response: {:?}", e);
                        return Failure(());
                    }
                }
            }

            if let Err(e) = stream.write_all(&buffer[..read]) {
                error_!("Stream write_all() failed: {:?}", e);
                return Failure(());
            }
        }

        if let Err(e) = stream.end() {
            error_!("Stream end() failed: {:?}", e);
            return Failure(());
        }

        Success(())
    }
}
