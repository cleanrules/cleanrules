//! The paths a rule set reads, as a tree of segments: the binding reads only
//! these in Erlang data (`spec/data-model.md`, *Reading native data*).

use cleanrules::Segment;

/// The paths a rule set reads, as a tree of segments, in the order the rule
/// set first reads them.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Paths {
    /// A path ends here: the whole value is needed.
    pub(crate) end: bool,
    /// String segments, each with the paths below it.
    pub(crate) keys: Vec<(String, Paths)>,
    /// Integer segments, each with the paths below it.
    pub(crate) indices: Vec<(u64, Paths)>,
}

impl Paths {
    /// The tree of `paths`.
    pub(crate) fn new<'a>(paths: impl IntoIterator<Item = &'a [Segment]>) -> Self {
        let mut root = Paths::default();
        for path in paths {
            root.add(path);
        }
        root
    }

    fn add(&mut self, path: &[Segment]) {
        let mut node = self;
        for segment in path {
            node = match segment {
                Segment::Key(key) => {
                    let at = node.keys.iter().position(|(k, _)| k == key);
                    let at = at.unwrap_or_else(|| {
                        node.keys.push((key.clone(), Paths::default()));
                        node.keys.len() - 1
                    });
                    &mut node.keys[at].1
                }
                Segment::Index(index) => {
                    let at = node.indices.iter().position(|(i, _)| i == index);
                    let at = at.unwrap_or_else(|| {
                        node.indices.push((*index, Paths::default()));
                        node.indices.len() - 1
                    });
                    &mut node.indices[at].1
                }
            };
        }
        node.end = true;
    }
}

/// Dropped with an explicit stack: a path may have any number of segments,
/// and a scheduler thread's stack is small.
impl Drop for Paths {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        take_children(self, &mut pending);
        while let Some(mut paths) = pending.pop() {
            take_children(&mut paths, &mut pending);
        }
    }
}

fn take_children(paths: &mut Paths, pending: &mut Vec<Paths>) {
    pending.extend(std::mem::take(&mut paths.keys).into_iter().map(|(_, p)| p));
    pending.extend(
        std::mem::take(&mut paths.indices)
            .into_iter()
            .map(|(_, p)| p),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str) -> Segment {
        Segment::Key(k.to_owned())
    }

    #[test]
    fn shared_prefixes_share_a_branch_in_first_order() {
        let a = vec![key("customer"), key("age")];
        let b = vec![key("customer"), key("tier")];
        let c = vec![key("items"), Segment::Index(0), key("price")];
        let d = vec![key("customer")];
        let paths = Paths::new([a.as_slice(), b.as_slice(), c.as_slice(), d.as_slice()]);
        assert!(!paths.end);
        let names: Vec<&str> = paths.keys.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["customer", "items"]);
        let customer = &paths.keys[0].1;
        assert!(customer.end);
        let fields: Vec<&str> = customer.keys.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(fields, ["age", "tier"]);
        assert!(customer.keys.iter().all(|(_, p)| p.end));
        let items = &paths.keys[1].1;
        assert_eq!(items.indices.len(), 1);
        assert_eq!(items.indices[0].0, 0);
        assert!(items.indices[0].1.keys[0].1.end);
    }

    #[test]
    fn the_empty_path_is_the_whole_input() {
        let paths = Paths::new([[].as_slice()]);
        assert!(paths.end);
        assert!(paths.keys.is_empty() && paths.indices.is_empty());
        assert_eq!(Paths::new([]), Paths::default());
    }

    #[test]
    fn long_paths_drop_on_a_small_stack() {
        let path: Vec<Segment> = (0..200_000)
            .map(|i| {
                if i % 2 == 0 {
                    Segment::Index(0)
                } else {
                    key("k")
                }
            })
            .collect();
        let dropped = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || drop(Paths::new([path.as_slice()])))
            .unwrap()
            .join();
        assert!(dropped.is_ok());
    }

    #[test]
    fn repeated_indices_share_a_branch() {
        let a = vec![Segment::Index(2)];
        let b = vec![Segment::Index(2), key("x")];
        let paths = Paths::new([a.as_slice(), b.as_slice()]);
        assert_eq!(paths.indices.len(), 1);
        assert!(paths.indices[0].1.end);
        assert_eq!(paths.indices[0].1.keys.len(), 1);
    }
}
