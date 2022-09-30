use crate::circuits::{
    constraints::ConstraintSystem,
    gate::{CircuitGate, CircuitGateError, GateType},
    polynomial::COLUMNS,
    polynomials::foreign_field_add::witness::{create_witness, FFOps},
    wires::Wire,
};
use ark_ec::AffineCurve;
use ark_ff::{One, Zero};
use mina_curves::pasta::{Pallas, Vesta};
use num_bigint::BigUint;
use num_traits::FromPrimitive;
use o1_utils::{
    foreign_field::{ForeignElement, HI, LO, MI, SECP256K1_MOD},
    FieldHelpers,
};

type PallasField = <Pallas as AffineCurve>::BaseField;
type VestaField = <Vesta as AffineCurve>::BaseField;

// Maximum value in the foreign field of secp256k1
// BigEndian -> FFFFFFFF FFFFFFFF FFFFFFFF FFFFFFFF FFFFFFFF FFFFFFFF FFFFFFFE FFFFFC2E
static MAX_SECP256K1: &[u8] = &[
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFF, 0xFF, 0xFC, 0x2E,
];

// A value that produces a negative low carry when added to itself
static OVF_NEG_LO: &[u8] = &[
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// A value that produces a negative middle carry when added to itself
static OVF_NEG_MI: &[u8] = &[
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFF, 0xFF, 0xFC, 0x2E,
];

// A value that produces overflow but the high limb of the result is smaller than the high limb of the modulus
static OVF_LESS_HI_LEFT: &[u8] = &[
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFF, 0xFF, 0xFC, 0x2E,
];
static OVF_LESS_HI_RIGHT: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x03, 0xD1,
];

// A value that produces two negative carries when added together with [OVF_ZERO_MI_NEG_LO]
static OVF_NEG_BOTH: &[u8] = &[
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// A value that produces two negative carries when added to itself with a middle limb that is all zeros
static OVF_ZERO_MI_NEG_LO: &[u8] = &[
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// All 0x55 bytes meaning [0101 0101]
static TIC: &[u8] = &[
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
];

// Prefix 0xAA bytes but fits in foreign field (suffix is zeros)
static TOC: &[u8] = &[
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// Bytestring that produces carry in low limb when added to TIC
static TOC_LO: &[u8] = &[
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xA9, 0xBA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// Bytestring that produces carry in mid limb when added to TIC
static TOC_MI: &[u8] = &[
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xA9, 0xBA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// Bytestring that produces carry in low and mid limb when added to TIC
static TOC_TWO: &[u8] = &[
    0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xA9, 0xBA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
    0xAA, 0xAA, 0xAA, 0xAA, 0xA9, 0xBA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// BigEndian -> 00000000 00000000 00000000 00000000 FFFFFFFF FFFFFFFF FFFFFFFE FFFFFC2F
// Bottom half of the foreign modulus
static FOR_MOD_BOT: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFF, 0xFF, 0xFC, 0x2F,
];

// BigEndian -> FFFFFFFF FFFFFFFF FFFFFFFF FFFFFFFF 00000000 00000000 00000000 00000000
// Top half of the foreign modulus
static FOR_MOD_TOP: &[u8] = &[
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// Value that performs a + - 1 low carry when added to [MAX]
static NULL_CARRY_LO: &[u8] = &[0x01, 0x00, 0x00, 0x03, 0xD2];

// Value that performs a + - 1 middle carry when added to [MAX]
static NULL_CARRY_MI: &[u8] = &[
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];

// Value that performs two + - 1 carries when added to [MAX]
static NULL_CARRY_BOTH: &[u8] = &[
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x03, 0xD2,
];
// The zero byte
static ZERO: &[u8] = &[0x00];

// The one byte
static ONE: &[u8] = &[0x01];

fn create_test_constraint_system_ffadd(
    num: usize,
    modulus: BigUint,
) -> ConstraintSystem<PallasField> {
    let (mut next_row, mut gates) = CircuitGate::<PallasField>::create_foreign_field_add(0, num);

    // Temporary workaround for lookup-table/domain-size issue
    for _ in 0..(1 << 13) {
        gates.push(CircuitGate::zero(Wire::new(next_row)));
        next_row += 1;
    }

    ConstraintSystem::create(gates)
        .foreign_field_modulus(&Some(modulus))
        .build()
        .unwrap()
}

// returns the maximum value for a field of modulus size
fn field_max(modulus: BigUint) -> BigUint {
    modulus - 1u32
}

// helper to reduce lines of code in repetitive test structure
fn test_ffadd(
    fmod: &[u8],
    inputs: Vec<&[u8]>,
    ops: &Vec<FFOps>,
) -> ([Vec<PallasField>; COLUMNS], ConstraintSystem<PallasField>) {
    let nops = ops.len();
    let foreign_modulus = BigUint::from_bytes_be(fmod);
    let cs = create_test_constraint_system_ffadd(nops, foreign_modulus.clone());
    let inputs = inputs
        .iter()
        .map(|x| BigUint::from_bytes_be(x))
        .collect::<Vec<BigUint>>();
    let witness = create_witness(&inputs, ops, foreign_modulus);

    let all_rows = witness[0].len();

    for row in 0..all_rows {
        assert_eq!(
            cs.gates[row].verify_witness::<Vesta>(
                row,
                &witness,
                &cs,
                &witness[0][0..cs.public].to_vec()
            ),
            Ok(())
        );
    }

    // the row structure from the end will be: n ffadds, 1 final add, 1 final zero
    let add_row = all_rows - nops - 2;

    for row in add_row..all_rows {
        assert_eq!(
            cs.gates[row].verify::<Vesta>(row, &witness, &cs, &[]),
            Ok(())
        );
    }

    (witness, cs)
}

// checks that the result cells of the witness are computed as expected
fn check_result(witness: [Vec<PallasField>; COLUMNS], result: Vec<ForeignElement<PallasField, 3>>) {
    let add_row = witness[0].len() - 1 - result.len();
    for (idx, res) in result.iter().enumerate() {
        assert_eq!(witness[0][add_row + idx], res[LO]);
        assert_eq!(witness[1][add_row + idx], res[MI]);
        assert_eq!(witness[2][add_row + idx], res[HI]);
    }
}

fn check_ovf(witness: [Vec<PallasField>; COLUMNS], ovf: PallasField) {
    let ovf_row = witness[0].len() - 3;
    assert_eq!(witness[7][ovf_row], ovf);
}

fn check_carry(witness: [Vec<PallasField>; COLUMNS], lo: PallasField, mi: PallasField) {
    let carry_row = witness[0].len() - 3;
    assert_eq!(witness[8][carry_row], lo);
    assert_eq!(witness[9][carry_row], mi);
}

#[test]
// Add zero to zero. This checks that small amounts also get packed into limbs
fn test_zero_add() {
    test_ffadd(SECP256K1_MOD, vec![ZERO, ZERO], &vec![FFOps::Add]);
}

#[test]
// Adding terms that are zero modulo the foreign field
fn test_zero_sum_foreign() {
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![FOR_MOD_BOT, FOR_MOD_TOP],
        &vec![FFOps::Add],
    );
    check_result(witness, vec![ForeignElement::zero()]);
}

#[test]
// Adding terms that are zero modulo the native field
fn test_zero_sum_native() {
    let native_modulus = PallasField::modulus_biguint();
    let one = BigUint::new(vec![1u32]);
    let mod_minus_one = native_modulus.clone() - one.clone();
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![ONE, &mod_minus_one.to_bytes_be()],
        &vec![FFOps::Add],
    );

    // Check result is the native modulus
    let native_limbs = ForeignElement::<PallasField, 3>::from_biguint(native_modulus);
    check_result(witness, vec![native_limbs]);
}

#[test]
fn test_one_plus_one() {
    let (witness, _cs) = test_ffadd(SECP256K1_MOD, vec![ONE, ONE], &vec![FFOps::Add]);
    // check result is 2
    let two = ForeignElement::from_be(&[2]);
    check_result(witness, vec![two]);
}

#[test]
// Adds two terms that are the maximum value in the foreign field
fn test_max_number() {
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![MAX_SECP256K1, MAX_SECP256K1],
        &vec![FFOps::Add],
    );

    // compute result in the foreign field after taking care of the exceeding bits
    let sum = BigUint::from_bytes_be(MAX_SECP256K1) + BigUint::from_bytes_be(MAX_SECP256K1);
    let sum_mod = sum - BigUint::from_bytes_be(SECP256K1_MOD);
    let sum_mod_limbs = ForeignElement::<PallasField, 3>::from_biguint(sum_mod);
    check_ovf(witness.clone(), PallasField::one());
    check_result(witness, vec![sum_mod_limbs]);
}

#[test]
// test 0 - 1 where (-1) is in the foreign field
// this is tested first as 0 + neg(1)
// and then as 0 - 1
// and it is checked that in both cases the result is the same
fn test_zero_minus_one() {
    // FIRST AS NEG
    let foreign_modulus = BigUint::from_bytes_be(SECP256K1_MOD);
    let right_be_neg = ForeignElement::<PallasField, 3>::from_be(ONE)
        .neg(&foreign_modulus)
        .to_big()
        .to_bytes_be();
    let right_for_neg: ForeignElement<PallasField, 3> = ForeignElement::from_be(&right_be_neg);
    let (witness_neg, _cs) =
        test_ffadd(SECP256K1_MOD, vec![ZERO, &right_be_neg], &vec![FFOps::Add]);
    check_result(witness_neg, vec![right_for_neg.clone()]);

    // NEXT AS SUB
    let (witness_sub, _cs) = test_ffadd(SECP256K1_MOD, vec![ZERO, ONE], &vec![FFOps::Sub]);
    check_result(witness_sub, vec![right_for_neg]);
}

#[test]
// test 1 - 1 + 1 where (-1) is in the foreign field
// the first check is done with sub(1, 1) and then with add(neg(neg(1)))
fn test_one_minus_one_plus_one() {
    let foreign_modulus = BigUint::from_bytes_be(SECP256K1_MOD);
    let neg_neg_one = ForeignElement::<PallasField, 3>::from_be(ONE)
        .neg(&foreign_modulus)
        .neg(&foreign_modulus)
        .to_big()
        .to_bytes_be();
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![ONE, ONE, &neg_neg_one],
        &vec![FFOps::Sub, FFOps::Add],
    );

    // intermediate 1 - 1 should be zero
    // final 0 + 1 should be 1
    check_result(
        witness,
        vec![ForeignElement::zero(), ForeignElement::from_be(ONE)],
    );
}

#[test]
// test -1-1 where (-1) is in the foreign field
// first tested as neg(1) + neg(1)
// then tested as 0 - 1 - 1 )
// TODO tested as 0 - ( 1 + 1) -> put sign in front of left instead
fn test_minus_minus() {
    let foreign_modulus = BigUint::from_bytes_be(SECP256K1_MOD);
    let neg_one_for = ForeignElement::<PallasField, 3>::from_be(ONE).neg(&foreign_modulus);
    let neg_one = neg_one_for.to_big().to_bytes_be();
    let neg_two = ForeignElement::<PallasField, 3>::from_biguint(BigUint::from_u32(2).unwrap())
        .neg(&foreign_modulus);
    let (witness_neg, _cs) = test_ffadd(SECP256K1_MOD, vec![&neg_one, &neg_one], &vec![FFOps::Add]);
    check_result(witness_neg, vec![neg_two.clone()]);

    let (witness_sub, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![ZERO, ONE, ONE],
        &vec![FFOps::Sub, FFOps::Sub],
    );
    check_result(witness_sub, vec![neg_one_for, neg_two]);
}

#[test]
// test when the low carry is minus one
fn test_neg_carry_lo() {
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![OVF_NEG_LO, OVF_NEG_LO],
        &vec![FFOps::Add],
    );
    check_carry(witness, -PallasField::one(), PallasField::zero());
}

#[test]
// test when the middle carry is minus one
fn test_neg_carry_mi() {
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![OVF_NEG_MI, OVF_NEG_MI],
        &vec![FFOps::Add],
    );
    check_carry(witness, PallasField::zero(), -PallasField::one());
}

#[test]
// test when there is negative low carry and 0 middle limb (carry bit propagates)
fn test_propagate_carry() {
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![OVF_ZERO_MI_NEG_LO, OVF_ZERO_MI_NEG_LO],
        &vec![FFOps::Add],
    );
    check_carry(witness, -PallasField::one(), -PallasField::one());
}

#[test]
// test when the both carries are minus one
fn test_neg_carries() {
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![OVF_NEG_BOTH, OVF_ZERO_MI_NEG_LO],
        &vec![FFOps::Add],
    );
    check_carry(witness, -PallasField::one(), -PallasField::one());
}

#[test]
// test the upperbound of the result
fn test_upperbound() {
    test_ffadd(
        SECP256K1_MOD,
        vec![OVF_LESS_HI_LEFT, OVF_LESS_HI_RIGHT],
        &vec![FFOps::Add],
    );
}

#[test]
// test a carry that nullifies in the low limb
fn test_null_lo_carry() {
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![MAX_SECP256K1, NULL_CARRY_LO],
        &vec![FFOps::Add],
    );
    check_carry(witness, PallasField::zero(), PallasField::zero());
}

#[test]
// test a carry that nullifies in the mid limb
fn test_null_mi_carry() {
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![MAX_SECP256K1, NULL_CARRY_MI],
        &vec![FFOps::Add],
    );
    check_carry(witness, PallasField::zero(), PallasField::zero());
}

#[test]
// test a carry that nullifies in the mid limb
fn test_null_both_carry() {
    let (witness, _cs) = test_ffadd(
        SECP256K1_MOD,
        vec![MAX_SECP256K1, NULL_CARRY_BOTH],
        &vec![FFOps::Add],
    );
    check_carry(witness, PallasField::zero(), PallasField::zero());
}

#[test]
// test sums without carry bits in any limb
fn test_no_carry_limbs() {
    let (witness, _cs) = test_ffadd(SECP256K1_MOD, vec![TIC, TOC], &vec![FFOps::Add]);
    check_carry(witness.clone(), PallasField::zero(), PallasField::zero());
    // check middle limb is all ones
    let all_one_limb = PallasField::from(2u128.pow(88) - 1);
    assert_eq!(witness[1][17], all_one_limb);
}

#[test]
// test sum with carry only in low part
fn test_pos_carry_limb_lo() {
    let (witness, _cs) = test_ffadd(SECP256K1_MOD, vec![TIC, TOC_LO], &vec![FFOps::Add]);
    check_carry(witness, PallasField::one(), PallasField::zero());
}

#[test]
fn test_pos_carry_limb_mid() {
    let (witness, _cs) = test_ffadd(SECP256K1_MOD, vec![TIC, TOC_MI], &vec![FFOps::Add]);
    check_carry(witness, PallasField::zero(), PallasField::one());
}

#[test]
fn test_pos_carry_limb_lo_mid() {
    let (witness, _cs) = test_ffadd(SECP256K1_MOD, vec![TIC, TOC_TWO], &vec![FFOps::Add]);
    check_carry(witness, PallasField::one(), PallasField::one());
}

#[test]
// Check it fails if given a wrong result
fn test_wrong_sum() {
    let (mut witness, cs) = test_ffadd(SECP256K1_MOD, vec![TIC, TOC], &vec![FFOps::Add]);
    // wrong result
    let all_ones_limb = PallasField::from(2u128.pow(88) - 1);
    witness[0][8] = all_ones_limb.clone();
    witness[0][17] = all_ones_limb.clone();

    assert_eq!(
        cs.gates[16].verify_foreign_field_add::<Vesta>(0, &witness, &cs),
        Err(CircuitGateError::InvalidConstraint(
            GateType::ForeignFieldAdd
        )),
    );
}

#[test]
// Test subtraction of the foreign field
fn test_zero_sub_fmod() {
    let (witness, _cs) = test_ffadd(SECP256K1_MOD, vec![ZERO, SECP256K1_MOD], &vec![FFOps::Sub]);
    // -f should be 0 mod f
    check_result(witness, vec![ForeignElement::zero()]);
}

#[test]
// Test subtraction of the foreign field maximum value
fn test_zero_sub_fmax() {
    let (witness, _cs) = test_ffadd(SECP256K1_MOD, vec![ZERO, MAX_SECP256K1], &vec![FFOps::Sub]);
    let foreign_modulus = BigUint::from_bytes_be(SECP256K1_MOD);
    let negated = ForeignElement::<PallasField, 3>::from_be(MAX_SECP256K1).neg(&foreign_modulus);
    check_result(witness, vec![negated]);
}

// The order of the Pallas curve is 0x40000000000000000000000000000000224698fc0994a8dd8c46eb2100000001.
// The order of the Vesta curve is  0x40000000000000000000000000000000224698fc094cf91b992d30ed00000001.

#[test]
// Test with Pasta curves where foreign field is smaller than the native field
fn test_pasta_add_max_vesta() {
    let vesta_modulus = VestaField::modulus_biguint();
    let vesta_mod_be = vesta_modulus.to_bytes_be();
    let right_input = field_max(vesta_modulus.clone());
    let (witness, _cs) = test_ffadd(
        &vesta_mod_be,
        vec![ZERO, &right_input.to_bytes_be()],
        &vec![FFOps::Add],
    );
    let right = right_input % vesta_modulus;
    let right_foreign = ForeignElement::<PallasField, 3>::from_biguint(right);
    check_result(witness, vec![right_foreign]);
}

#[test]
// Test with Pasta curves where foreign field is smaller than the native field
fn test_pasta_sub_max_vesta() {
    let vesta_modulus = VestaField::modulus_biguint();
    let vesta_mod_be = vesta_modulus.to_bytes_be();
    let right_input = field_max(vesta_modulus.clone());
    let (witness, _cs) = test_ffadd(
        &vesta_mod_be,
        vec![ZERO, &right_input.to_bytes_be()],
        &vec![FFOps::Sub],
    );
    let neg_max_vesta =
        ForeignElement::<PallasField, 3>::from_biguint(right_input).neg(&vesta_modulus);
    check_result(witness, vec![neg_max_vesta]);
}

#[test]
// Test with Pasta curves where foreign field is smaller than the native field
fn test_pasta_add_max_pallas() {
    let vesta_modulus = VestaField::modulus_biguint();
    let vesta_mod_be = vesta_modulus.to_bytes_be();
    let right_input = field_max(PallasField::modulus_biguint());
    let (witness, _cs) = test_ffadd(
        &vesta_mod_be,
        vec![ZERO, &right_input.to_bytes_be()],
        &vec![FFOps::Add],
    );
    let right = right_input % vesta_modulus;
    let foreign_right = ForeignElement::<PallasField, 3>::from_biguint(right);
    check_result(witness, vec![foreign_right]);
}

#[test]
// Test with Pasta curves where foreign field is smaller than the native field
fn test_pasta_sub_max_pallas() {
    let vesta_modulus = VestaField::modulus_biguint();
    let vesta_mod_be = vesta_modulus.to_bytes_be();
    let right_input = field_max(PallasField::modulus_biguint());
    let (witness, _cs) = test_ffadd(
        &vesta_mod_be,
        vec![ZERO, &right_input.to_bytes_be()],
        &vec![FFOps::Sub],
    );
    let neg_max_pallas =
        ForeignElement::<PallasField, 3>::from_biguint(right_input).neg(&vesta_modulus);
    check_result(witness, vec![neg_max_pallas]);
}
