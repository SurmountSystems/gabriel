use byteorder::{LittleEndian, WriteBytesExt};
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug)]
pub struct TransactionInput {
    pub previous_output_txid: [u8; 32],
    pub previous_output_vout: u32,
    pub script: Vec<u8>,
    pub sequence: u32,
}

#[derive(Debug)]
pub struct TransactionOutput {
    pub value: u64,
    pub script: Vec<u8>,
}

#[derive(Debug)]
pub struct Transaction {
    pub version: u32,
    pub inputs: Vec<TransactionInput>,
    pub outputs: Vec<TransactionOutput>,
    pub lock_time: u32,
}

impl TransactionInput {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Write the previous output hash
        writer.write_all(&self.previous_output_txid)?;

        // Write the previous output index (u32)
        writer.write_u32::<LittleEndian>(self.previous_output_vout)?;

        // Write the script length (VarInt)
        write_varint(writer, self.script.len() as u64)?;

        // Write the script itself
        writer.write_all(&self.script)?;

        // Write the sequence (u32)
        writer.write_u32::<LittleEndian>(self.sequence)?;

        Ok(())
    }
}

impl TransactionOutput {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Write the value (u64)
        writer.write_u64::<LittleEndian>(self.value)?;

        // Write the script length (VarInt)
        write_varint(writer, self.script.len() as u64)?;

        // Write the script itself
        writer.write_all(&self.script)?;

        Ok(())
    }
}

impl Transaction {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Write the version (u32)
        writer.write_u32::<LittleEndian>(self.version)?;

        // Write the number of inputs (VarInt)
        write_varint(writer, self.inputs.len() as u64)?;

        // Serialize each input
        for input in &self.inputs {
            input.serialize(writer)?;
        }

        // Write the number of outputs (VarInt)
        write_varint(writer, self.outputs.len() as u64)?;

        // Serialize each output
        for output in &self.outputs {
            output.serialize(writer)?;
        }

        // Write the lock_time (u32)
        writer.write_u32::<LittleEndian>(self.lock_time)?;

        Ok(())
    }

    pub fn txid(&self) -> [u8; 32] {
        // Serialize the transaction
        let mut serialized_tx = Vec::new();
        self.serialize(&mut serialized_tx).unwrap();

        // Perform double SHA-256 hashing
        let hash1 = Sha256::digest(&serialized_tx);
        let hash2 = Sha256::digest(hash1);

        // Convert the hash into a fixed-size array
        let mut txid = [0u8; 32];
        txid.copy_from_slice(&hash2);
        txid
    }
}

// Helper function to write a VarInt (used for Bitcoin serialization)
fn write_varint<W: Write>(writer: &mut W, value: u64) -> std::io::Result<()> {
    if value < 0xFD {
        writer.write_u8(value as u8)?;
    } else if value <= 0xFFFF {
        writer.write_u8(0xFD)?;
        writer.write_u16::<LittleEndian>(value as u16)?;
    } else if value <= 0xFFFF_FFFF {
        writer.write_u8(0xFE)?;
        writer.write_u32::<LittleEndian>(value as u32)?;
    } else {
        writer.write_u8(0xFF)?;
        writer.write_u64::<LittleEndian>(value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_creation_and_txid() {
        let input = TransactionInput {
            previous_output_txid: [0u8; 32],
            previous_output_vout: 0,
            script: vec![0x6a, 0x14],
            sequence: 0xFFFFFFFF,
        };

        let output = TransactionOutput {
            value: 50_000_000,
            script: vec![0x76, 0xa9, 0x14],
        };

        let tx = Transaction {
            version: 1,
            inputs: vec![input],
            outputs: vec![output],
            lock_time: 0,
        };

        let txid = tx.txid();

        // Assert that the txid is not all zeros (this is a basic check)
        assert_ne!(txid, [0u8; 32]);
    }
}
