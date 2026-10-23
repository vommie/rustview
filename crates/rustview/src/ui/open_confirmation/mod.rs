//! Confirmation before an "Open" action opens many files at once (Settings > General > Confirmations).
//!
//! Every "Open" entry except the Multiview (which has its own tile limit) goes through [`OpenConfirmation::run_or_ask`]: below the
//! configured count the action runs at once, otherwise the dialog asks first and runs it on "Continue". The dialog lives in a
//! thread-local (UI thread only), so no controller has to be passed through the menu contexts.

mod dialog;
pub mod policy;

pub use dialog::OpenConfirmation;
pub use policy::OpenKind;
