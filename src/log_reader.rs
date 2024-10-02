use super::common::*;
use rev_buf_reader::RevBufReader;
use std::fs::{self};
use std::io::{self, BufRead, Read, Seek};

pub struct LogReader<'a> {
    rev_reader: RevBufReader<&'a mut fs::File>,
}

impl<'a> LogReader<'a> {
    pub fn new(file: &mut fs::File) -> Result<LogReader, io::Error> {
        let rev_reader = RevBufReader::new(file);
        Ok(LogReader { rev_reader })
    }

    fn read_record(&mut self) -> Result<Option<Record>, io::Error> {
        if self.rev_reader.stream_position()? == 0 {
            return Ok(None);
        }

        // Check that the record starts with the record separator
        if self.read_special_sequence()? != SpecialSequence::RecordSeparator {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Record candidate does not end with record separator",
            ));
        }

        // The buffer that stores all the bytes of the record read so far in reverse order.
        let mut result_buf: Vec<u8> = Vec::new();
        // The buffer that stores the bytes read from the file.
        let mut read_buf: Vec<u8> = Vec::new();

        loop {
            read_buf.clear();
            self.rev_reader
                .read_until(ESCAPE_CHARACTER, &mut read_buf)?;

            result_buf.extend(read_buf.iter().rev());

            if self.rev_reader.stream_position()? == 0 {
                // If we've reached the beginning of the file, we've read the entire record.
                break;
            }

            // Otherwise, we must have encountered an escape character.
            match self.read_special_sequence()? {
                SpecialSequence::RecordSeparator => {
                    // The record is complete, so we can break out of the loop.
                    // Move the cursor back to the beginning of the special sequence.
                    self.rev_reader.seek_relative(3)?;
                    break;
                }
                SpecialSequence::LiteralFieldSeparator => {
                    // The field separator is escaped, so we need to add it to the result buffer.
                    result_buf.push(FIELD_SEPARATOR);
                }
                SpecialSequence::LiteralEscape => {
                    // The escape character is escaped, so we need to add it to the result buffer.
                    result_buf.push(ESCAPE_CHARACTER);
                }
            }
        }

        result_buf.reverse();
        let record = Record::deserialize(&result_buf);
        Ok(Some(record))
    }

    fn read_special_sequence(&mut self) -> Result<SpecialSequence, io::Error> {
        let mut special_buf: Vec<u8> = vec![0; 3];
        self.rev_reader.read_exact(&mut special_buf)?;

        match validate_special(&special_buf.as_slice()) {
            Some(special) => Ok(special),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Not a special sequence",
            )),
        }
    }
}

impl Iterator for LogReader<'_> {
    type Item = Record;

    fn next(&mut self) -> Option<Self::Item> {
        match self.read_record() {
            Ok(Some(record)) => Some(record),
            Ok(None) => None,
            Err(err) => panic!("Error reading record: {:?}", err),
        }
    }
}

/// There are three special characters that need to be handled:
/// Here: SC = escape char, FS = field separator.
/// - FS FS SC  -> actual record separator
/// - SC FS SC  -> literal FS
/// - SC SC SC  -> literal SC
fn validate_special(buf: &[u8]) -> Option<SpecialSequence> {
    match buf {
        SEQ_RECORD_SEP => Some(SpecialSequence::RecordSeparator),
        SEQ_LIT_FIELD_SEP => Some(SpecialSequence::LiteralFieldSeparator),
        SEQ_LIT_ESCAPE => Some(SpecialSequence::LiteralEscape),
        _ => None,
    }
}