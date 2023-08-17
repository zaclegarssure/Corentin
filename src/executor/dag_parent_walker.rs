/// Copyright warning, this code has been adapthed from
/// [daggy](https://github.com/mitchmindtree/daggy).
/// Bla bla bla, all rights belongs to their original authors something something.


use std::marker::PhantomData;

use petgraph::{prelude::DiGraph, stable_graph::IndexType, visit::Walker, graph::{NodeIndex, EdgeIndex}};

/// A **Walker** type that can be used to step through the parents of some child node.
pub struct Parents<N, E, Ix: IndexType> {
    walk_edges: petgraph::graph::WalkNeighbors<Ix>,
    _node: PhantomData<N>,
    _edge: PhantomData<E>,
}

/// A **Walker** type that may be used to step through the parents of the given child node.
///
/// Unlike iterator types, **Walker**s do not require borrowing the internal **Graph**. This
/// makes them useful for traversing the **Graph** while still being able to mutably borrow it.
///
/// If you require an iterator, use one of the **Walker** methods for converting this
/// **Walker** into a similarly behaving **Iterator** type.
///
/// See the [**Walker**](Walker) trait for more useful methods.
pub fn parents<N, E, Ix: IndexType>(graph: DiGraph<N, E, Ix>, child: NodeIndex<Ix>) -> Parents<N, E, Ix> {
    let walk_edges = graph.neighbors_directed(child, petgraph::Incoming).detach();
    Parents {
        walk_edges,
        _node: PhantomData,
        _edge: PhantomData,
    }
}

impl<'a, N, E, Ix> Walker<&'a DiGraph<N, E, Ix>> for Parents<N, E, Ix>
where
    Ix: IndexType,
{
    type Item = (EdgeIndex<Ix>, NodeIndex<Ix>);
    #[inline]
    fn walk_next(&mut self, dag: &'a DiGraph<N, E, Ix>) -> Option<Self::Item> {
        self.walk_edges.next(&dag)
    }
}
