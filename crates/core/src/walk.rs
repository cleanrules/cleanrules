//! Depth-first walks with an explicit stack.
//!
//! Conditions, result trees and data values nest as deep as the reader's limits
//! allow, and bindings call the core from threads with small stacks (a JVM
//! thread has 1 MiB, and a debug build's frames are several times larger than a
//! release build's). Walks over those trees keep their state on the heap, so the
//! call stack they need does not grow with the depth (`spec/json-format.md`,
//! *Errors and limits*).

use std::vec::Drain;

/// Walk a tree depth first, as a recursive function would, with an explicit stack.
///
/// `next(node, done)` gives the node's next child once `done` of its children
/// are finished, or `None` when it has no more. `finish(node, results)` makes the
/// node's result from its children's results, in order (drained from the walk's
/// stack of results, so taking them allocates nothing more). So `next` meets
/// every node in pre-order, before its subtree, and `finish` in post-order; a
/// check made when a child is created in `next` is made where a recursive walk
/// would make it on entering that child. The first error stops the walk.
pub(crate) fn post_order<T, R, E>(
    root: T,
    next: impl FnMut(&mut T, usize) -> Result<Option<T>, E>,
    finish: impl FnMut(T, Drain<'_, R>) -> Result<R, E>,
) -> Result<R, E> {
    Walk::default().run(root, next, finish)
}

/// The stacks of [`post_order`], kept to be used again: walks made one after
/// another with the same `Walk` allocate their stacks once, such as the walks
/// over every rule of a rule set.
pub(crate) struct Walk<T, R> {
    /// The results of the finished children of the nodes being walked.
    results: Vec<R>,
    /// The ancestors of the node being walked, each with where its children's
    /// results start in `results`.
    ancestors: Vec<(T, usize)>,
}

impl<T, R> Default for Walk<T, R> {
    fn default() -> Self {
        Self {
            results: Vec::new(),
            ancestors: Vec::new(),
        }
    }
}

impl<T, R> Walk<T, R> {
    /// [`post_order`], with these stacks.
    pub(crate) fn run<E>(
        &mut self,
        root: T,
        mut next: impl FnMut(&mut T, usize) -> Result<Option<T>, E>,
        mut finish: impl FnMut(T, Drain<'_, R>) -> Result<R, E>,
    ) -> Result<R, E> {
        // A walk that failed leaves its stacks as they were.
        self.results.clear();
        self.ancestors.clear();
        // The node being walked, with where its children's results start.
        let mut current: (T, usize) = (root, 0);
        loop {
            if let Some(child) = next(&mut current.0, self.results.len() - current.1)? {
                let start = self.results.len();
                self.ancestors
                    .push(std::mem::replace(&mut current, (child, start)));
                continue;
            }
            let (node, start) = current;
            let result = finish(node, self.results.drain(start..))?;
            match self.ancestors.pop() {
                Some(parent) => {
                    self.results.push(result);
                    current = parent;
                }
                None => return Ok(result),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::post_order;

    /// A tree as nested vectors: each node is its children.
    #[derive(Debug)]
    struct Tree(Vec<Tree>);

    fn chain(depth: usize) -> Tree {
        let mut tree = Tree(Vec::new());
        for _ in 1..depth {
            tree = Tree(vec![tree]);
        }
        tree
    }

    /// A labelled tree.
    struct Labelled(char, Vec<Labelled>);

    #[test]
    fn orders_are_those_of_recursion() {
        // a(b(d), c): pre-order a b d c, post-order d b c a.
        let leaf = |label| Labelled(label, Vec::new());
        let tree = Labelled('a', vec![Labelled('b', vec![leaf('d')]), leaf('c')]);
        let mut pre = Vec::new();
        let post: Result<String, ()> = post_order(
            &tree,
            |node, done| {
                if done == 0 {
                    pre.push(node.0);
                }
                Ok(node.1.get(done))
            },
            |node, children| Ok(format!("{}{}", children.collect::<String>(), node.0)),
        );
        assert_eq!(pre, ['a', 'b', 'd', 'c']);
        assert_eq!(post.unwrap(), "dbca");
    }

    #[test]
    fn the_first_error_stops_the_walk() {
        let tree = Tree(vec![Tree(vec![]), Tree(vec![]), Tree(vec![])]);
        let mut finished = 0;
        let result: Result<(), usize> = post_order(
            &tree,
            |node, done| Ok(node.0.get(done)),
            |node, _| {
                finished += 1;
                if node.0.is_empty() && finished == 2 {
                    Err(finished)
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result, Err(2));
        assert_eq!(finished, 2);
    }

    #[test]
    fn a_walk_is_used_again_after_an_error() {
        let tree = Tree(vec![Tree(vec![]), Tree(vec![])]);
        let mut walk = super::Walk::default();
        let mut count = |fail: bool| {
            walk.run(
                &tree,
                |node, done| Ok(node.0.get(done)),
                |node, children| {
                    if fail && node.0.is_empty() {
                        Err(())
                    } else {
                        Ok(1 + children.sum::<usize>())
                    }
                },
            )
        };
        assert_eq!(count(true), Err(()));
        assert_eq!(count(false), Ok(3));
    }

    #[test]
    fn depth_needs_no_stack() {
        // Far deeper than any limit, on a small stack.
        let depth = 1_000_000;
        let height = std::thread::Builder::new()
            .stack_size(64 << 10)
            .spawn(move || {
                let tree = chain(depth);
                let height: Result<usize, ()> = post_order(
                    &tree,
                    |node, done| Ok(node.0.get(done)),
                    |_, children| Ok(1 + children.max().unwrap_or(0)),
                );
                // Unrolled, since this test tree's own drop is recursive.
                let mut tree = tree;
                while let Some(child) = tree.0.pop() {
                    tree = child;
                }
                height
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(height, Ok(depth));
    }
}
