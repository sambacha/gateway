/// Interest rate related calculations and utilities are concentrated here
use codec::{Decode, Encode};
use our_std::{Deserialize, RuntimeDebug, Serialize};

use crate::{
    params::MILLISECONDS_PER_YEAR,
    reason::{MathError, Reason},
    types::{uint_from_string_with_decimals, AssetAmount, CashIndex, Timestamp, Uint},
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// Error enum for interest rates
#[derive(Copy, Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub enum RatesError {
    UtilizationZeroSupplyError,
    UtilizationBorrowedIsMoreThanSupplied,
    ModelNotIncreasingZeroRateAboveKinkRate,
    ModelNotIncreasingKinkRateAboveFullRate,
    ModelKinkUtilizationOver100Percent,
    ModelKinkUtilizationNotPositive,
    ModelRateOutOfBounds,
    Overflowed,
    ReserveFactorOver100Percent,
}

/// Annualized interest rate
#[derive(Serialize, Deserialize)] // used in config
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug)]
pub struct APR(pub Uint);

impl From<Uint> for APR {
    fn from(x: u128) -> Self {
        APR(x)
    }
}

impl APR {
    pub const DECIMALS: u8 = 4;
    pub const ZERO: APR = APR::from_nominal("0");
    pub const ONE: APR = APR::from_nominal("1");
    pub const MAX: APR = APR::from_nominal("0.35"); // 35%

    pub const fn from_nominal(s: &'static str) -> Self {
        APR(uint_from_string_with_decimals(Self::DECIMALS, s))
    }

    /// exp{r * dt} where dt is change in time in seconds
    // XXX why is this an index, should it be a CashIndexDelta or something?
    pub fn over_time(self, dt: Timestamp) -> Result<CashIndex, MathError> {
        let index_scale = &BigInt::from(CashIndex::ONE.0);
        let scaled_rate: &BigInt =
            &(index_scale * self.0 * dt / MILLISECONDS_PER_YEAR / APR::ONE.0);
        let t1 = index_scale * index_scale * index_scale; //     1
        let t2 = scaled_rate * index_scale * index_scale; //     x
        let t3 = scaled_rate * scaled_rate * index_scale / 2; // x^2 / 2
        let t4 = scaled_rate * scaled_rate * scaled_rate / 6; // x^3 / 6
        let unscaled = t1 + t2 + t3 + t4;
        let scaled: BigInt = unscaled / index_scale / index_scale;
        if let Some(raw) = scaled.to_u128() {
            Ok(CashIndex(raw))
        } else {
            Err(MathError::Overflow)
        }
    }
}

impl Default for APR {
    fn default() -> Self {
        APR(0)
    }
}

impl our_std::str::FromStr for APR {
    type Err = Reason;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        Ok(APR(u128::from_str(string).map_err(|_| Reason::InvalidAPR)?))
    }
}

impl From<APR> for String {
    fn from(string: APR) -> Self {
        format!("{}", string.0)
    }
}

/// XXX rename this
#[derive(Serialize, Deserialize)] // used in config
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug)]
pub struct ReserveFactor(Uint);

impl From<Uint> for ReserveFactor {
    fn from(x: u128) -> Self {
        ReserveFactor(x)
    }
}

impl ReserveFactor {
    pub const DECIMALS: u8 = 4;
    pub const ZERO: ReserveFactor = ReserveFactor::from_nominal("0");
    pub const ONE: ReserveFactor = ReserveFactor::from_nominal("1");

    pub const fn from_nominal(s: &'static str) -> Self {
        ReserveFactor(uint_from_string_with_decimals(Self::DECIMALS, s))
    }
}

impl Default for ReserveFactor {
    fn default() -> Self {
        ReserveFactor::ZERO
    }
}

/// Utilization rate for a given market.
#[derive(Serialize, Deserialize)] // used in config
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug)]
pub struct Utilization(Uint);

impl From<Uint> for Utilization {
    fn from(x: u128) -> Self {
        Utilization(x)
    }
}

impl Utilization {
    pub const DECIMALS: u8 = 4;
    pub const ZERO: Utilization = Utilization::from_nominal("0");
    pub const ONE: Utilization = Utilization::from_nominal("1");

    pub const fn from_nominal(s: &'static str) -> Self {
        Utilization(uint_from_string_with_decimals(Self::DECIMALS, s))
    }
}

/// Internal function for getting a raw utilization. Used so that we can use ? operator with options
/// then write one ok_or later.
fn get_raw_utilization(supplied: AssetAmount, borrowed: AssetAmount) -> Option<Uint> {
    borrowed
        .checked_mul(Utilization::ONE.0)?
        .checked_div(supplied)
}

/// Get the utilization ratio given the amount supplied and borrowed. These amounts should be in
/// "today" money AKA "balance" money.
pub fn get_utilization(
    supplied: AssetAmount,
    borrowed: AssetAmount,
) -> Result<Utilization, RatesError> {
    if supplied == 0 && borrowed == 0 {
        // 0 over 0 is defined to be 0 utilization
        return Ok(Utilization::ZERO);
    }

    if borrowed > supplied {
        return Err(RatesError::UtilizationBorrowedIsMoreThanSupplied);
    }

    let result = get_raw_utilization(supplied, borrowed).ok_or(RatesError::Overflowed)?;

    Ok(result.into())
}

/// This represents an interest rate model type and parameters.
/// It also implements the pure functionality of the interest rate model
/// leaving out all issues of storage read and write.
///
/// In the future we may support serde serialization and deserialization of this struct
/// for the purpose of inclusion in the genesis configuration chain_spec file.
#[derive(Serialize, Deserialize)] // used in config
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug)]
pub enum InterestRateModel {
    Kink {
        zero_rate: APR,
        kink_rate: APR,
        kink_utilization: Utilization,
        full_rate: APR,
    },
}

/// This is _required_ for storage, we should never depend on this.
impl Default for InterestRateModel {
    fn default() -> Self {
        Self::new_kink(0, 500, 8000, 2000)
    }
}

impl InterestRateModel {
    /// Create a new kink model.
    pub fn new_kink<T, U, W, V>(
        zero_rate: T,
        kink_rate: U,
        kink_utilization: W,
        full_rate: V,
    ) -> InterestRateModel
    where
        T: Into<APR>,
        U: Into<APR>,
        V: Into<APR>,
        W: Into<Utilization>,
    {
        InterestRateModel::Kink {
            zero_rate: zero_rate.into(),
            kink_rate: kink_rate.into(),
            kink_utilization: kink_utilization.into(),
            full_rate: full_rate.into(),
        }
    }

    /// Check the model parameters for sanity
    ///
    /// Kink - we expect the kink model to be monotonically increasing with a kink somewhere between
    /// 0% and 100% utilization
    pub fn check_parameters(self: &Self) -> Result<(), RatesError> {
        match self {
            Self::Kink {
                zero_rate,
                kink_rate,
                kink_utilization,
                full_rate,
            } => {
                if *zero_rate > APR::MAX || *kink_rate > APR::MAX || *full_rate > APR::MAX {
                    return Err(RatesError::ModelRateOutOfBounds);
                }

                if zero_rate >= kink_rate {
                    return Err(RatesError::ModelNotIncreasingZeroRateAboveKinkRate);
                }

                if kink_rate >= full_rate {
                    return Err(RatesError::ModelNotIncreasingKinkRateAboveFullRate);
                }

                if *kink_utilization >= Utilization::ONE {
                    return Err(RatesError::ModelKinkUtilizationOver100Percent);
                }

                if *kink_utilization <= Utilization::ZERO {
                    return Err(RatesError::ModelKinkUtilizationNotPositive);
                }
            }
        };

        Ok(())
    }

    /// The left side of the kink in the kink model.
    fn left_line(
        utilization: Uint,
        zero_rate: Uint,
        kink_rate: Uint,
        kink_utilization: Uint,
        _full_rate: Uint,
    ) -> Option<Uint> {
        // utilization * (kink_rate - zero_rate) / kink_utilization + zero_rate
        utilization
            .checked_mul(kink_rate.checked_sub(zero_rate)?)?
            .checked_div(kink_utilization)?
            .checked_add(zero_rate)
    }

    /// The right side of the kink in the kink model.
    fn right_line(
        utilization: Uint,
        _zero_rate: Uint,
        kink_rate: Uint,
        kink_utilization: Uint,
        full_rate: Uint,
    ) -> Option<Uint> {
        // (utilization - kink_utilization)*(full_rate - kink_rate) / ( 1 - kink_utilization) + kink_rate
        utilization
            .checked_sub(kink_utilization)?
            .checked_mul(full_rate.checked_sub(kink_rate)?)?
            .checked_div(Utilization::ONE.0.checked_sub(kink_utilization)?)?
            .checked_add(kink_rate)
    }

    /// Get the borrow rate
    /// Current rate is not used at the moment
    pub fn get_borrow_rate<T: Into<APR>>(
        self: &Self,
        utilization: Utilization,
        _current_rate: T,
    ) -> Result<APR, RatesError> {
        match self {
            Self::Kink {
                zero_rate,
                kink_rate,
                kink_utilization,
                full_rate,
            } => {
                if utilization < *kink_utilization {
                    let result = Self::left_line(
                        utilization.0,
                        zero_rate.0,
                        kink_rate.0,
                        kink_utilization.0,
                        full_rate.0,
                    )
                    .ok_or(RatesError::Overflowed)?;

                    return Ok(result.into());
                } else {
                    let result = Self::right_line(
                        utilization.0,
                        zero_rate.0,
                        kink_rate.0,
                        kink_utilization.0,
                        full_rate.0,
                    )
                    .ok_or(RatesError::Overflowed)?;

                    return Ok(result.into());
                }
            }
        };
    }

    fn borrow_rate_to_supply_rate(
        borrow_rate: Uint,
        reserve_factor: Uint,
        utilization: Uint,
    ) -> Result<Uint, MathError> {
        // Borrow Rate * (1-reserve factor) * utilization

        // (1-reserve factor)
        let reserve_multiplier = ReserveFactor::ONE
            .0
            .checked_sub(reserve_factor)
            .ok_or(MathError::Underflow)?;

        // Borrow Rate * (1-reserve factor)
        let acc = crate::types::mul(
            borrow_rate,
            APR::DECIMALS,
            reserve_multiplier,
            ReserveFactor::DECIMALS,
            APR::DECIMALS,
        )?;

        // Borrow Rate * (1-reserve factor) * utilization
        let acc = crate::types::mul(
            acc,
            APR::DECIMALS,
            utilization,
            Utilization::DECIMALS,
            APR::DECIMALS,
        )?;

        Ok(acc)
    }

    /// Get the (borrow_rate, supply_rate) pair, they're often needed at the same time.
    pub fn get_rates(
        self: &Self,
        utilization: Utilization,
        current_rate: APR,
        reserve_factor: ReserveFactor,
    ) -> Result<(APR, APR), RatesError> {
        let borrow_rate = self.get_borrow_rate(utilization, current_rate)?;
        // unsafe version Borrow Rate * (1-reserve factor) * utilization
        let supply_rate =
            Self::borrow_rate_to_supply_rate(borrow_rate.0, reserve_factor.0, utilization.0)
                .map_err(|_| RatesError::Overflowed)?;
        Ok((borrow_rate, APR(supply_rate)))
    }

    /// Get the supply rate
    ///
    /// always Borrow Rate * (1-reserve factor) * utilization
    pub fn get_supply_rate(
        self: &Self,
        utilization: Utilization,
        current_rate: APR,
        reserve_factor: ReserveFactor,
    ) -> Result<APR, RatesError> {
        let (_, supply_rate) = self.get_rates(utilization, current_rate, reserve_factor)?;
        Ok(supply_rate)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    struct UtilizationTestCase {
        supplied: AssetAmount,
        borrowed: AssetAmount,
        expected: Result<Utilization, RatesError>,
        message: &'static str,
    }

    struct InterestRateModelCheckParametersTestCase {
        model: InterestRateModel,
        expected: Result<(), RatesError>,
        message: &'static str,
    }

    struct InterestRateModelGetBorrowRateTestCase {
        model: InterestRateModel,
        utilization: Utilization,
        expected: Result<APR, RatesError>,
        message: &'static str,
    }

    fn get_utilization_test_cases() -> Vec<UtilizationTestCase> {
        vec![
            UtilizationTestCase {
                supplied: 0,
                borrowed: 0,
                expected: Ok(Utilization::ZERO),
                message: "Zero supply and zero borrow is defined as zero utilization",
            },
            UtilizationTestCase {
                supplied: 0,
                borrowed: 1,
                expected: Err(RatesError::UtilizationBorrowedIsMoreThanSupplied),
                message: "Borrowed can not be more than supplied, even when supplied is zero",
            },
            UtilizationTestCase {
                supplied: 1,
                borrowed: 2,
                expected: Err(RatesError::UtilizationBorrowedIsMoreThanSupplied),
                message: "Borrowed can not be more than supplied",
            },
            UtilizationTestCase {
                supplied: Uint::max_value(),
                borrowed: Uint::max_value(),
                expected: Err(RatesError::Overflowed),
                message: "These numbers are vastly too large to compute the utilization",
            },
            UtilizationTestCase {
                supplied: Uint::max_value() / Utilization::ONE.0 + 1,
                borrowed: Uint::max_value() / Utilization::ONE.0 + 1,
                expected: Err(RatesError::Overflowed),
                message: "These numbers are only just too large to compute the utilization",
            },
            UtilizationTestCase {
                supplied: Uint::max_value() / Utilization::ONE.0,
                borrowed: Uint::max_value() / Utilization::ONE.0,
                expected: Ok(Utilization::ONE),
                message: "These are the largest numbers we can use to compute the utilization",
            },
            UtilizationTestCase {
                supplied: 100,
                borrowed: 100,
                expected: Ok(Utilization::ONE),
                message: "This is a basic test of 100% utilization",
            },
            UtilizationTestCase {
                supplied: 100,
                borrowed: 0,
                expected: Ok(0.into()),
                message: "A basic test of 0 utilization",
            },
            UtilizationTestCase {
                supplied: 100,
                borrowed: 50,
                expected: Ok(Utilization::from_nominal("0.5")),
                message: "A basic test of middling utilization",
            },
        ]
    }

    fn test_get_utilizatio_case(case: UtilizationTestCase) {
        assert_eq!(
            case.expected,
            get_utilization(case.supplied, case.borrowed),
            "{}",
            case.message
        );
    }

    #[test]
    fn test_get_utilization() {
        get_utilization_test_cases()
            .drain(..)
            .for_each(test_get_utilizatio_case)
    }

    fn get_check_parameters_test_cases() -> Vec<InterestRateModelCheckParametersTestCase> {
        vec![
            InterestRateModelCheckParametersTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 1.into(),
                    kink_rate: 2.into(),
                    full_rate: 3.into(),
                    kink_utilization: Utilization::from_nominal("0.5"),
                },
                expected: Ok(()),
                message: "typical case should work well",
            },
            InterestRateModelCheckParametersTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 1.into(),
                    kink_rate: 1.into(),
                    full_rate: 3.into(),
                    kink_utilization: Utilization::from_nominal("0.5"),
                },
                expected: Err(RatesError::ModelNotIncreasingZeroRateAboveKinkRate),
                message: "rates must be increasing between zero and kink",
            },
            InterestRateModelCheckParametersTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 1.into(),
                    kink_rate: 2.into(),
                    full_rate: 2.into(),
                    kink_utilization: Utilization::from_nominal("0.5"),
                },
                expected: Err(RatesError::ModelNotIncreasingKinkRateAboveFullRate),
                message: "rates must be increasing between kink and 100% util rate",
            },
            InterestRateModelCheckParametersTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 1.into(),
                    kink_rate: 2.into(),
                    full_rate: 3.into(),
                    kink_utilization: Utilization::ONE,
                },
                expected: Err(RatesError::ModelKinkUtilizationOver100Percent),
                message: "kink must be less than 100%",
            },
            InterestRateModelCheckParametersTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 1.into(),
                    kink_rate: 2.into(),
                    full_rate: 3.into(),
                    kink_utilization: 0.into(),
                },
                expected: Err(RatesError::ModelKinkUtilizationNotPositive),
                message: "kink must be more than zero",
            },
            InterestRateModelCheckParametersTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: APR(APR::MAX.0 + 1),
                    kink_rate: 2.into(),
                    full_rate: 3.into(),
                    kink_utilization: 0.into(),
                },
                expected: Err(RatesError::ModelRateOutOfBounds),
                message: "rate must be less than max rate",
            },
            InterestRateModelCheckParametersTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 1.into(),
                    kink_rate: APR(APR::MAX.0 + 1),
                    full_rate: 3.into(),
                    kink_utilization: 0.into(),
                },
                expected: Err(RatesError::ModelRateOutOfBounds),
                message: "rate must be less than max rate",
            },
            InterestRateModelCheckParametersTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 1.into(),
                    kink_rate: 2.into(),
                    full_rate: APR(APR::MAX.0 + 1),
                    kink_utilization: 0.into(),
                },
                expected: Err(RatesError::ModelRateOutOfBounds),
                message: "rate must be less than max rate",
            },
        ]
    }

    fn test_check_parameters_case(case: InterestRateModelCheckParametersTestCase) {
        assert_eq!(
            case.model.check_parameters(),
            case.expected,
            "{}",
            case.message
        );
    }

    #[test]
    fn test_check_parameters() {
        get_check_parameters_test_cases()
            .drain(..)
            .for_each(test_check_parameters_case)
    }

    fn get_get_borrow_rate_test_cases() -> Vec<InterestRateModelGetBorrowRateTestCase> {
        // sheet for working out these test cases https://docs.google.com/spreadsheets/d/1s7mASgM2Jlz0sKd7oMRVIujf56QlhiXLQfCg0Tr9dAY/edit?usp=sharing
        vec![
            InterestRateModelGetBorrowRateTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 100.into(),
                    kink_rate: 200.into(),
                    kink_utilization: 5000.into(),
                    full_rate: 500.into(),
                },
                utilization: 0.into(),
                expected: Ok(100.into()),
                message: "rate at zero utilization should be zero utilization rate",
            },
            InterestRateModelGetBorrowRateTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 100.into(),
                    kink_rate: 200.into(),
                    kink_utilization: 5000.into(),
                    full_rate: 500.into(),
                },
                utilization: 5000.into(),
                expected: Ok(200.into()),
                message: "rate at kink utilization should be kink utilization rate",
            },
            InterestRateModelGetBorrowRateTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 100.into(),
                    kink_rate: 200.into(),
                    kink_utilization: 5000.into(),
                    full_rate: 500.into(),
                },
                utilization: Utilization::ONE,
                expected: Ok(500.into()),
                message: "rate at full utilization should be full utilization rate",
            },
            InterestRateModelGetBorrowRateTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 100.into(),
                    kink_rate: 200.into(),
                    kink_utilization: 5000.into(),
                    full_rate: 500.into(),
                },
                utilization: 1000.into(),
                expected: Ok(120.into()),
                message: "rate at point between zero and kink",
            },
            InterestRateModelGetBorrowRateTestCase {
                model: InterestRateModel::Kink {
                    zero_rate: 100.into(),
                    kink_rate: 200.into(),
                    kink_utilization: 5000.into(),
                    full_rate: 500.into(),
                },
                utilization: 8000.into(),
                expected: Ok(380.into()),
                message: "rate at point between kink and full",
            },
        ]
    }

    fn test_get_borrow_rate_case(case: InterestRateModelGetBorrowRateTestCase) {
        assert_eq!(
            case.expected,
            case.model.get_borrow_rate(case.utilization, 0),
            "{}",
            case.message
        )
    }

    #[test]
    fn test_get_borrow_rate() {
        get_get_borrow_rate_test_cases()
            .drain(..)
            .for_each(test_get_borrow_rate_case)
    }

    #[test]
    fn test_over_time() {
        let mut rates = vec!["0", "0.0001", "0.03", "0.1", "0.2"];
        // XXX should positively assert some failures here instead of commenting these out?
        // let months_per_year = 12;
        // let weeks_per_year = 52;
        let days_per_year = 365;
        let hours_per_year = days_per_year * 24;
        let minutes_per_year = hours_per_year * 60;
        let seconds_per_year = minutes_per_year * 60;

        let year_fractions = vec![
            // months_per_year,
            // weeks_per_year,
            // days_per_year,
            hours_per_year,
            minutes_per_year,
            seconds_per_year,
        ];

        for rate in rates.drain(..) {
            for year_frac in year_fractions.iter() {
                let r = APR::from_nominal(rate);
                let dt = MILLISECONDS_PER_YEAR / year_frac;
                let actual = match r.over_time(dt) {
                    Ok(actual) => actual,
                    Err(e) => panic!(
                        "Math error during over_time  r = {}, year_frac = {}, error = {:?}",
                        rate, year_frac, e
                    ),
                };

                let float_rate = (r.0 as f64) / 10f64.powf(APR::DECIMALS as f64);
                let float_rate_over_time =
                    float_rate * (dt as f64) / (MILLISECONDS_PER_YEAR as f64);
                let float_exact_reference = float_rate_over_time.exp();
                let float_exact_as_uint =
                    (float_exact_reference * (CashIndex::ONE.0 as f64)) as u128;
                let error_wei = if float_exact_as_uint > actual.0 {
                    float_exact_as_uint - actual.0
                } else {
                    actual.0 - float_exact_as_uint
                };

                assert!(error_wei < 1000, "exp test case out of range");
                // println!(
                //     "{}, {}, {}, {}",
                //     rate,
                //     year_frac,
                //     actual.0,
                //     float_exact_reference * (CashIndex::ONE.0 as f64)
                // );
            }
        }
    }
}
