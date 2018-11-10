#![forbid(unsafe_code, bad_style, future_incompatible)]
#![forbid(rust_2018_idioms, rust_2018_compatibility)]
#![deny(missing_debug_implementations)]
#![forbid(missing_docs)]
#![cfg_attr(test, deny(warnings))]

//! Key-value storage, with a refresh and a time-to-live system.
//!
//! A k-buckets table allows one to store a value identified by keys, ordered by
//! their distance to a reference key passed to the constructor.
//!
//! If the local ID has `N` bits, then the k-buckets table contains `N`
//! *buckets* each containing a constant number of entries. Storing a key in the
//! k-buckets table adds it to the bucket corresponding to its distance with the
//! reference key.

use arrayvec::ArrayVec;
use bigint::U512;
use parking_lot::{Mutex, MutexGuard};
use std::mem;
use std::slice::Iter as SliceIter;
use std::time::{Duration, Instant};
use std::vec::IntoIter as VecIntoIter;

/// Maximum number of nodes in a bucket.
pub const MAX_NODES_PER_BUCKET: usize = 20;

#[derive(Debug, Clone, Eq ,PartialEq)]
struct PeerId {}

impl PeerId {
  fn digest(&self) -> &[u8] {
    unimplemented!();
  }
}

/// Table of k-buckets with interior mutability.
#[derive(Debug)]
pub struct KBucketsTable<Id, Val> {
  my_id: Id,
  tables: Vec<Mutex<KBucket<Id, Val>>>,
  ping_timeout: Duration,
}

#[derive(Debug, Clone)]
struct KBucket<Id, Val> {
  nodes: ArrayVec<[Node<Id, Val>; MAX_NODES_PER_BUCKET]>,
  pending_node: Option<(Node<Id, Val>, Instant)>,
  last_update: Instant,
}

impl<Id, Val> KBucket<Id, Val> {
  fn flush(&mut self, timeout: Duration) {
    if let Some((pending_node, instant)) = self.pending_node.take() {
      if instant.elapsed() >= timeout {
        let _ = self.nodes.remove(0);
        self.nodes.push(pending_node);
      } else {
        self.pending_node = Some((pending_node, instant));
      }
    }
  }
}

#[derive(Debug, Clone)]
struct Node<Id, Val> {
  id: Id,
  value: Val,
}

/// Trait that must be implemented on types that can be used as an identifier in
/// a k-bucket.
pub trait KBucketsPeerId: Eq + Clone {
  /// Distance between two peer IDs.
  type Distance: Ord;

  /// Computes the XOR of this value and another one.
  fn distance_with(&self, other: &Self) -> Self::Distance;

  /// Returns then number of bits that are necessary to store the distance
  /// between peer IDs.  Used for pre-allocations.
  ///
  /// > **Note**: Returning 0 would lead to a panic.
  fn num_bits() -> usize;

  /// Returns the number of leading zeroes of the distance between peer IDs.
  fn leading_zeros(distance: Self::Distance) -> u32;
}

impl KBucketsPeerId for PeerId {
  type Distance = U512;

  #[inline]
  fn num_bits() -> usize {
    512
  }

  #[inline]
  fn distance_with(&self, other: &Self) -> Self::Distance {
    // Note that we don't compare the hash functions because there's no chance of collision
    // of the same value hashed with two different hash functions.
    let my_hash = U512::from(self.digest());
    let other_hash = U512::from(other.digest());
    my_hash ^ other_hash
  }

  #[inline]
  fn leading_zeros(distance: Self::Distance) -> u32 {
    distance.leading_zeros()
  }
}

impl<Id, Val> KBucketsTable<Id, Val>
where
  Id: KBucketsPeerId,
{
  /// Builds a new routing table.
  pub fn new(my_id: Id, ping_timeout: Duration) -> Self {
    KBucketsTable {
      my_id: my_id,
      tables: (0..Id::num_bits())
        .map(|_| KBucket {
          nodes: ArrayVec::new(),
          pending_node: None,
          last_update: Instant::now(),
        }).map(Mutex::new)
        .collect(),
      ping_timeout: ping_timeout,
    }
  }

  // Returns the id of the bucket that should contain the peer with the given ID.
  //
  // Returns `None` if out of range, which happens if `id` is the same as the local peer id.
  #[inline]
  fn bucket_num(&self, id: &Id) -> Option<usize> {
    (Id::num_bits() - 1)
      .checked_sub(Id::leading_zeros(self.my_id.distance_with(id)) as usize)
  }

  /// Returns an iterator to all the buckets of this table.
  ///
  /// Ordered by proximity to the local node. Closest bucket (with max. one node in it) comes
  /// first.
  #[inline]
  pub fn buckets(&self) -> BucketsIter<'_, Id, Val> {
    BucketsIter(self.tables.iter(), self.ping_timeout)
  }

  /// Returns the ID of the local node.
  #[inline]
  pub fn my_id(&self) -> &Id {
    &self.my_id
  }

  /// Finds the `num` nodes closest to `id`, ordered by distance.
  pub fn find_closest(&self, id: &Id) -> VecIntoIter<Id>
  where
    Id: Clone,
  {
    // TODO: optimize
    let mut out = Vec::new();
    for table in self.tables.iter() {
      let mut table = table.lock();
      table.flush(self.ping_timeout);
      if table.last_update.elapsed() > self.ping_timeout {
        continue; // ignore bucket with expired nodes
      }
      for node in table.nodes.iter() {
        out.push(node.id.clone());
      }
    }
    out.sort_by(|a, b| b.distance_with(id).cmp(&a.distance_with(id)));
    out.into_iter()
  }

  /// Same as `find_closest`, but includes the local peer as well.
  pub fn find_closest_with_self(&self, id: &Id) -> VecIntoIter<Id>
  where
    Id: Clone,
  {
    // TODO: optimize
    let mut intermediate: Vec<_> = self.find_closest(&id).collect();
    if let Some(pos) = intermediate
      .iter()
      .position(|e| e.distance_with(&id) >= self.my_id.distance_with(&id))
    {
      if intermediate[pos] != self.my_id {
        intermediate.insert(pos, self.my_id.clone());
      }
    } else {
      intermediate.push(self.my_id.clone());
    }
    intermediate.into_iter()
  }

  /// Marks the node as "most recent" in its bucket and modifies the value associated to it.
  /// This function should be called whenever we receive a communication from a node.
  pub fn update(&self, id: Id, value: Val) -> UpdateOutcome<Id, Val> {
    let table = match self.bucket_num(&id) {
      Some(n) => &self.tables[n],
      None => return UpdateOutcome::FailSelfUpdate,
    };

    let mut table = table.lock();
    table.flush(self.ping_timeout);

    if let Some(pos) = table.nodes.iter().position(|n| n.id == id) {
      // Node is already in the bucket.
      let mut existing = table.nodes.remove(pos);
      let old_val = mem::replace(&mut existing.value, value);
      if pos == 0 {
        // If it's the first node of the bucket that we update, then we drop the node that
        // was waiting for a ping.
        table.nodes.truncate(MAX_NODES_PER_BUCKET - 1);
        table.pending_node = None;
      }
      table.nodes.push(existing);
      table.last_update = Instant::now();
      UpdateOutcome::Refreshed(old_val)
    } else if table.nodes.len() < MAX_NODES_PER_BUCKET {
      // Node not yet in the bucket, but there's plenty of space.
      table.nodes.push(Node {
        id: id,
        value: value,
      });
      table.last_update = Instant::now();
      UpdateOutcome::Added
    } else {
      // Not enough space to put the node, but we can add it to the end as "pending". We
      // then need to tell the caller that we want it to ping the node at the top of the
      // list.
      if table.pending_node.is_none() {
        table.pending_node = Some((
          Node {
            id: id,
            value: value,
          },
          Instant::now(),
        ));
        UpdateOutcome::NeedPing(table.nodes[0].id.clone())
      } else {
        UpdateOutcome::Discarded
      }
    }
  }
}

/// Return value of the `update()` method.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[must_use]
pub enum UpdateOutcome<Id, Val> {
  /// The node has been added to the bucket.
  Added,
  /// The node was already in the bucket and has been refreshed.
  Refreshed(Val),
  /// The node wasn't added. Instead we need to ping the node passed as parameter, and call
  /// `update` if it responds.
  NeedPing(Id),
  /// The node wasn't added at all because a node was already pending.
  Discarded,
  /// Tried to update the local peer ID. This is an invalid operation.
  FailSelfUpdate,
}

/// Iterator giving access to a bucket.
#[derive(Debug, Clone)]
pub struct BucketsIter<'a, Id, Val>(
  SliceIter<'a, Mutex<KBucket<Id, Val>>>,
  Duration,
);

impl<'a, Id, Val> Iterator for BucketsIter<'a, Id, Val> {
  type Item = Bucket<'a, Id, Val>;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    self.0.next().map(|bucket| {
      let mut bucket = bucket.lock();
      bucket.flush(self.1);
      Bucket(bucket)
    })
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    self.0.size_hint()
  }
}

impl<'a, Id: 'a, Val: 'a> ExactSizeIterator for BucketsIter<'a, Id, Val> {}

/// Access to a bucket.
#[allow(missing_debug_implementations)]
pub struct Bucket<'a, Id, Val>(MutexGuard<'a, KBucket<Id, Val>>);

impl<'a, Id: 'a, Val: 'a> Bucket<'a, Id, Val> {
  /// Returns the number of entries in that bucket.
  ///
  /// > **Note**: Keep in mind that this operation can be racy. If `update()` is called on the
  /// >           table while this function is running, the `update()` may or may not be taken
  /// >           into account.
  #[inline]
  pub fn num_entries(&self) -> usize {
    self.0.nodes.len()
  }

  /// Returns true if this bucket has a pending node.
  #[inline]
  pub fn has_pending(&self) -> bool {
    self.0.pending_node.is_some()
  }

  /// Returns the time when any of the values in this bucket was last updated.
  ///
  /// If the bucket is empty, this returns the time when the whole table was created.
  #[inline]
  pub fn last_update(&self) -> Instant {
    self.0.last_update.clone()
  }
}
