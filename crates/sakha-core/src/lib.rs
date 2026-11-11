//! Ardon Studio's logic, without a GUI, so tests can drive it headless.
//!
//! * [`layout`] — Pro's four panes and their dividers.
//! * [`profile`] — Sakha's view of a data frame's variables.
//! * [`analysis`] — Sakha's menu: what each analysis asks for, the R2 code
//!   it runs, and how its result reads in plain words.
//! * [`script`] — the session script, Sakha's reproducible record.
//! * [`document`] — the `.sakha` and `.r2proj` files.
//! * [`tables`] — publishable (APA) tables gathering the same test run on
//!   different variables, from the numbers R2 returned; HTML and CSV.
//! * [`gview`] — Gview's editable graphs: plot code whose settings change.
//! * [`edit`] — the Data tab's edits (cells, compute, recode, filter, sort,
//!   delete) as R2 code.
//! * [`import`]— R2 code for loading data and changing a column's attributes.
//!
//! **Sakha computes nothing itself.** Every number comes from R2 functions
//! run by `r2 --ide`, so Sakha always agrees with the Console.

pub mod analysis;
pub mod document;
pub mod edit;
pub mod gview;
pub mod import;
pub mod layout;
pub mod profile;
pub mod script;
pub mod tables;
