#![cfg_attr(feature = "nightly", deny(missing_docs))]
#![cfg_attr(feature = "nightly", feature(external_doc))]
#![cfg_attr(feature = "nightly", doc(include = "../README.md"))]
#![cfg_attr(test, deny(warnings))]
#![cfg_attr(test, feature(plugin))]
#![cfg_attr(test, plugin(clippy))]

pub struct KBucket {}

impl KBucket {
  /// Create a new instance.
  pub fn new() -> Self {
    unimplemented!();
  }

  /// Default arbiter function for contacts with the same id. Uses
  /// contact.vectorClock to select which contact to update the k-bucket with.
  /// Contact with larger vectorClock field will be selected. If vectorClock is
  /// the same, candidat will be selected.
  pub fn arbiter(&self) {
    unimplemented!();
  }

  /// Default distance function. Finds the XOR distance between firstId and
  /// secondId.
  pub fn distance(&self) {
    unimplemented!();
  }

  /// Adds a contact to the k-bucket.
  pub fn add(&self) {
    unimplemented!();
  }

  /// Get the n closest contacts to the provided node id. "Closest" here means:
  /// closest according to the XOR metric of the contact node id.
  pub fn closest(&self) {
    unimplemented!();
  }

  /// Counts the total number of contacts in the tree.
  // Adapted from `.count()`.
  pub fn len(&self) -> usize {
    unimplemented!();
  }

  // Adapted from `.count()` - required by clippy.
  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }

  /// Retrieves the contact.
  pub fn get(&self) {
    unimplemented!();
  }

  /// The metadata method serves as a container that can be used by
  /// implementations using k-bucket. One example is storing a timestamp to
  /// indicate the last time when a node in the bucket was responding to a
  /// ping.
  pub fn metadata(&self) {
    unimplemented!();
  }

  /// Removes contact with the provided id.
  pub fn remove(&self) {
    unimplemented!();
  }

  /// Traverses the tree, putting all the contacts into one arraverses the tree,
  /// putting all the contacts into one vector.
  // Adapted from `.toArray()`.
  pub fn to_vec(&self) {
    unimplemented!();
  }
}
