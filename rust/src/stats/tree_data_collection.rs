
pub type TreeDataCollection<M> = BTreeMap<String, M>;
pub struct TreeDataCollection<M> {
    data: BTreeMap<String, M>,
    count: usize,
}
impl<M> TreeDataCollection<M> {
    pub fn new() -> Self {
        TreeDataCollection {
            data: BTreeMap::new(),
            count: 0,
        }
    }
    pub fn insert(&mut self, key: String, value: M) {
        self.data.insert(key, value);
        self.count += value.get_count();
    }
    pub fn get_count(&self) -> usize {
        self.count
    }
    pub fn get_data(&self) -> &BTreeMap<String, M> {
        &self.data
    }
    pub fn extend(&mut self, other: Self) {
        self.data.extend(other.data);
        self.count += other.count;
    }
}
impl<M> Default for TreeDataCollection<M> {
    fn default() -> Self {
        Self::new()
    }
}
impl<M> FromIterator<(String, M)> for TreeDataCollection<M> {
    fn from_iter<I: IntoIterator<Item = (String, M)>>(iter: I) -> Self {
        let mut data = BTreeMap::new();
        let mut count = 0;
        for (key, value) in iter {
            data.insert(key, value);
            count += value.get_count();
        }
        TreeDataCollection { data, count }
    }
}
impl<M> FromIterator<TreeDataCollection<M>> for TreeDataCollection<M> {
    fn from_iter<I: IntoIterator<Item = TreeDataCollection<M>>>(iter: I) -> Self {
        let mut data = BTreeMap::new();
        let mut count = 0;
        for value in iter {
            data.extend(value.data);
            count += value.count;
        }
        TreeDataCollection { data, count }
    }
}
