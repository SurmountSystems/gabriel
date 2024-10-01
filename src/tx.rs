use byteorder::{LittleEndian, WriteBytesExt};
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug, Ord, Eq, PartialOrd, PartialEq, Clone)]
pub struct TransactionInput {
    pub previous_output_txid: [u8; 32],
    pub previous_output_vout: u16,
    pub script: Vec<u8>,
    pub sequence: u32,
    #[allow(dead_code)]
    pub witness: Vec<WitnessItem>,
}

#[derive(Debug, Ord, Eq, PartialOrd, PartialEq, Clone)]
pub struct TransactionOutput {
    pub value: u64,
    pub script: Vec<u8>,
}

#[derive(Debug, Ord, Eq, PartialOrd, PartialEq, Clone)]
pub struct Transaction {
    pub version: u32,
    pub inputs: Vec<TransactionInput>,
    pub outputs: Vec<TransactionOutput>,
    pub lock_time: u32,
    pub txid: [u8; 32],
}

#[derive(Debug, Ord, Eq, PartialOrd, PartialEq, Clone)]
pub struct WitnessItem {
    #[allow(dead_code)]
    pub witness: Vec<u8>,
}

impl TransactionInput {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Write the previous output hash
        writer.write_all(&self.previous_output_txid)?;

        // Write the previous output index (u32)
        writer.write_u32::<LittleEndian>(self.previous_output_vout as u32)?;

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
        let mut previous_output_txid: [u8; 32] =
            hex::decode("0437cd7f8525ceed2324359c2d0ba26006d92d856a9c20fa0241106ee5a597c9")
                .unwrap()
                .try_into()
                .unwrap();
        previous_output_txid.reverse();

        let input = TransactionInput {
            previous_output_txid,
            previous_output_vout: 0,
            script: hex::decode("47304402204e45e16932b8af514961a1d3a1a25fdf3f4f7732e9d624c6c61548ab5fb8cd410220181522ec8eca07de4860a4acdd12909d831cc56cbbac4622082221a8768d1d0901").unwrap(),
            sequence: 0xFFFFFFFF,
            witness: vec![],
        };

        let output_1 = TransactionOutput {
            value: 1_000_000_000,
            script: hex::decode("4104ae1a62fe09c5f51b13905f07f06b99a2f7159b2225f374cd378d71302fa28414e7aab37397f554a7df5f142c21c1b7303b8a0626f1baded5c72a704f7e6cd84cac").unwrap(),
        };

        let output_2 = TransactionOutput {
            value: 4_000_000_000,
            script: hex::decode("410411db93e1dcdb8a016b49840f8c53bc1eb68a382e97b1482ecad7b148a6909a5cb2e0eaddfb84ccf9744464f82e160bfa9b8b64f9d4c03f999b8643f656b412a3ac").unwrap(),
        };

        let tx = Transaction {
            version: 1,
            inputs: vec![input],
            outputs: vec![output_1, output_2],
            lock_time: 0,
            txid: [0u8; 32],
        };

        let txid = tx.txid();

        assert_eq!(
            hex::encode(txid),
            "169e1e83e930853391bc6f35f605c6754cfead57cf8387639d3b4096c54f18f4" // f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16 in natural byte order
        );
    }
}
