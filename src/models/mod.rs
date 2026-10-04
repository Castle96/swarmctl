pub mod config;
pub mod context;
pub mod failover;
pub mod network;
pub mod node;
pub mod port;
pub mod secret;
pub mod service;
pub mod stack;
pub mod task;
pub mod volume;

use serde::Serialize;
use tabled::Tabled;

/// A renderable row that carries labels.
///
/// `get --show-labels` needs this uniformly, and implementing it explicitly is
/// what stops label formatting from drifting between commands — several
/// call sites used to build `"k=v, k=v"` inline from a `HashMap`, which was both
/// unsorted and formatted differently from `format_labels`.
pub trait RowLabels {
    /// Pre-rendered `k=v,k=v` label string.
    fn labels(&self) -> &str;
}

/// Implemented by row types that have a canonical `name` for `-o name`.
pub trait Named {
    fn row_name(&self) -> String;
}

/// Convenience bound for anything `render_rows` can print.
pub trait Renderable: Tabled + Serialize + RowLabels + Named {}
impl<T: Tabled + Serialize + RowLabels + Named> Renderable for T {}
