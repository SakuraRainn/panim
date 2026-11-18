//! Identifiers for personas and accounts, and the address of a persona key.

use std::fmt;

/// The identifier of a persona in the local store. Only this computer uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct PersonaId(pub u64);

/// The identifier of an account in the local store. Only this computer uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct AccountId(pub u64);

impl fmt::Display for PersonaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "persona-{}", self.0)
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "account-{}", self.0)
    }
}

/// The 20-byte address of a persona key. Other people know the persona by this address.
///
/// The text form is `0x` and 40 lowercase hexadecimal digits. The EIP-55 checksum form comes with the key code.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct PersonaAddress(pub [u8; 20]);

impl PersonaAddress {
    /// Reads the text form. The `0x` prefix is necessary. Uppercase and lowercase digits are both correct.
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix("0x")?.as_bytes();
        if digits.len() != 40 {
            return None;
        }
        let mut bytes = [0u8; 20];
        for (byte, pair) in bytes.iter_mut().zip(digits.chunks_exact(2)) {
            *byte = (hex_value(pair[0])? << 4) | hex_value(pair[1])?;
        }
        Some(Self(bytes))
    }
}

fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for PersonaAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("0x")?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for PersonaAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PersonaAddress({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_text_form_goes_both_ways() {
        let text = "0x00a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2ff";
        let address = PersonaAddress::parse(text).unwrap();
        assert_eq!(address.to_string(), text);
        assert_eq!(
            PersonaAddress::parse(&text.to_uppercase().replace("0X", "0x")),
            Some(address)
        );
    }

    #[test]
    fn address_refuses_incorrect_text() {
        assert_eq!(PersonaAddress::parse("00a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2ff"), None);
        assert_eq!(PersonaAddress::parse("0x00a1"), None);
        assert_eq!(
            PersonaAddress::parse("0xzza1b2c3d4e5f60718293a4b5c6d7e8f90a1b2ff"),
            None
        );
    }
}
