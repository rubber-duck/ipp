//! Small ordered output sets retain capacity across completed frames.

use ipp_core::OutputRef;

#[derive(Default, Debug, PartialEq, Eq)]
pub(in crate::services::render) struct OutputSelectionSet(Vec<OutputRef>);

impl Clone for OutputSelectionSet {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }

    fn clone_from(&mut self, other: &Self) {
        self.0.clone_from(&other.0);
    }
}

impl OutputSelectionSet {
    pub fn insert(&mut self, output: OutputRef) {
        if let Err(index) = self.0.binary_search(&output) {
            self.0.insert(index, output);
        }
    }

    pub fn remove(&mut self, output: &OutputRef) {
        if let Ok(index) = self.0.binary_search(output) {
            self.0.remove(index);
        }
    }

    pub fn retain(&mut self, predicate: impl FnMut(&OutputRef) -> bool) {
        self.0.retain(predicate);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn contains(&self, output: &OutputRef) -> bool {
        self.0.binary_search(output).is_ok()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, OutputRef> {
        self.0.iter()
    }
}

impl Extend<OutputRef> for OutputSelectionSet {
    fn extend<T: IntoIterator<Item = OutputRef>>(&mut self, values: T) {
        for value in values {
            self.insert(value);
        }
    }
}

impl<'a> IntoIterator for &'a OutputSelectionSet {
    type Item = &'a OutputRef;
    type IntoIter = std::slice::Iter<'a, OutputRef>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
