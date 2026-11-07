/// Marks on history lines, `a` to `z`, like vim's (MAX-164). Each view keeps its own: a line is
/// a history entry's index in line mode, a world row's in terminal mode. Several marks can be
/// on one line
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    lines: [Option<usize>; 26],
}

/// The slot of a mark's letter, none for anything but `a` to `z`
fn slot(letter: char) -> Option<usize> {
    letter
        .is_ascii_lowercase()
        .then(|| (letter as u8 - b'a') as usize)
}

fn letter(slot: usize) -> char {
    (b'a' + slot as u8) as char
}

impl Marks {
    /// Puts the mark on `line`, moving it when it was set; false for a letter out of `a`..`z`
    pub fn set(&mut self, letter: char, line: usize) -> bool {
        let Some(slot) = slot(letter) else {
            return false;
        };
        self.lines[slot] = Some(line);
        true
    }

    pub fn get(&self, letter: char) -> Option<usize> {
        self.lines[slot(letter)?]
    }

    /// Takes the mark off, returning its line
    pub fn remove(&mut self, letter: char) -> Option<usize> {
        self.lines[slot(letter)?].take()
    }

    /// The first letter with no mark
    pub fn first_free(&self) -> Option<char> {
        self.lines.iter().position(Option::is_none).map(letter)
    }

    /// The marks by line, then by letter
    pub fn sorted(&self) -> Vec<(char, usize)> {
        let mut marks: Vec<_> = (0..26)
            .filter_map(|slot| Some((letter(slot), self.lines[slot]?)))
            .collect();
        marks.sort_by_key(|&(letter, line)| (line, letter));
        marks
    }

    /// The closest mark above `line` (the first letter on that line), none when there's none
    pub fn prev(&self, line: usize) -> Option<(char, usize)> {
        let marks = self.sorted();
        let last_line = marks.iter().rev().find(|&&(_, at)| at < line)?.1;
        marks.into_iter().find(|&(_, at)| at == last_line)
    }

    /// The closest mark below `line` (the first letter on that line), none when there's none
    pub fn next(&self, line: usize) -> Option<(char, usize)> {
        self.sorted().into_iter().find(|&(_, at)| at > line)
    }

    /// The `evicted` oldest lines were dropped: the marks move up with their lines, the ones
    /// on dropped lines go
    pub fn rebase_after_eviction(&mut self, evicted: usize) {
        for line in &mut self.lines {
            *line = line.and_then(|line| line.checked_sub(evicted));
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
#[path = "../tests/app/marks.rs"]
mod tests;
