use thiserror::Error;

use crate::{ArithmeticError, DomainValidationError};

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CalculationError {
    #[error(transparent)]
    Arithmetic(#[from] ArithmeticError),
    #[error(transparent)]
    Validation(#[from] DomainValidationError),
    #[error("the calculation requires at least one observation")]
    EmptySample,
    #[error("probability must be between zero and one inclusive")]
    InvalidProbability,
    #[error("weighted calculations require one strictly positive weight per value")]
    InvalidWeights,
    #[error("the requested exact payment route is not supported by both legs")]
    IncompatibleRoute,
    #[error("the selected ads do not match the required buy/sell leg sides")]
    IncompatibleSides,
    #[error("at least one spread leg is not eligible for the full normalized quantity")]
    IneligibleLeg,
    #[error("net calculations are unavailable because at least one cost is unknown")]
    UnknownCosts,
    #[error("cost values and bounds must be non-negative and internally ordered")]
    InvalidCostProfile,
    #[error("allocation constraints are non-negative and minimum cannot exceed maximum")]
    InvalidAllocationRange,
    #[error("allocation candidates must have unique stable identifiers")]
    DuplicateAllocationId,
    #[error("sensitivity inputs must be positive, unique, and sorted ascending")]
    InvalidSensitivityAmounts,
}
