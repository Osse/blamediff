use std::fmt::{Display, Write};
use std::hash::Hash;
use std::ops::Range;

use gix::diff::blob::intern::{InternedInput, Interner, Token};
use gix::diff::blob::Sink;

/// A [`Sink`](crate::sink::Sink) that creates a textual diff
/// in the format typically output by git or gnu-diff if the `-u` option is used
pub struct UnifiedDiffBuilder<'a, T>
where
    T: Hash + Eq + Display,
{
    before: &'a [Token],
    after: &'a [Token],
    interner: &'a Interner<T>,

    pos: u32,
    before_hunk_start: u32,
    after_hunk_start: u32,
    before_hunk_len: u32,
    after_hunk_len: u32,

    buffer: String,
    dst: String,

    before_blob: crate::BlobData<'a>,
    after_blob: crate::BlobData<'a>,
}

impl<'a, T> UnifiedDiffBuilder<'a, T>
where
    T: Hash + Eq + Display,
{
    /// Create a new `UnifiedDiffBuilder` for the given `input`,
    /// that will return a [`String`](std::string::String).
    pub fn new(
        input: &'a InternedInput<T>,
        old_blob: crate::BlobData<'a>,
        new_blob: crate::BlobData<'a>,
    ) -> Self {
        Self {
            before_hunk_start: 0,
            after_hunk_start: 0,
            before_hunk_len: 0,
            after_hunk_len: 0,
            buffer: String::with_capacity(8),
            dst: String::new(),
            interner: &input.interner,
            before: &input.before,
            after: &input.after,
            pos: 0,
            before_blob: old_blob,
            after_blob: new_blob,
        }
    }

    fn print_tokens(&mut self, tokens: &[Token], prefix: char) {
        for &token in tokens {
            writeln!(&mut self.buffer, "{prefix}{}", self.interner[token]).unwrap();
        }
    }

    fn flush(&mut self) {
        if self.before_hunk_len == 0 && self.after_hunk_len == 0 {
            return;
        }

        let end = (self.pos + 3).min(self.before.len() as u32);
        self.update_pos(end, end);

        writeln!(
            &mut self.dst,
            "diff --git a/{0} b/{1}\nindex {2}..{3} 100644\n--- a/{0}\n+++ b/{1}\n@@ -{4},{5} +{6},{7} @@",
            self.before_blob.path,
            self.after_blob.path,
            self.before_blob.id.to_hex_with_len(7),
            self.after_blob.id.to_hex_with_len(7),
            self.before_hunk_start + 1,
            self.before_hunk_len,
            self.after_hunk_start + 1,
            self.after_hunk_len,
        )
        .unwrap();

        write!(&mut self.dst, "{}", &self.buffer).unwrap();
        self.buffer.clear();
        self.before_hunk_len = 0;
        self.after_hunk_len = 0
    }

    fn update_pos(&mut self, print_to: u32, move_to: u32) {
        self.print_tokens(&self.before[self.pos as usize..print_to as usize], ' ');
        let len = print_to - self.pos;
        self.pos = move_to;
        self.before_hunk_len += len;
        self.after_hunk_len += len;
    }
}

impl<T> Sink for UnifiedDiffBuilder<'_, T>
where
    T: Hash + Eq + Display,
{
    type Out = String;

    fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
        if before.start - self.pos > 6 {
            self.flush();
            self.pos = before.start - 3;
            self.before_hunk_start = self.pos;
            self.after_hunk_start = after.start - 3;
        }
        self.update_pos(before.start, before.end);
        self.before_hunk_len += before.end - before.start;
        self.after_hunk_len += after.end - after.start;
        self.print_tokens(
            &self.before[before.start as usize..before.end as usize],
            '-',
        );
        self.print_tokens(&self.after[after.start as usize..after.end as usize], '+');
    }

    fn finish(mut self) -> Self::Out {
        self.flush();
        self.dst
    }
}
