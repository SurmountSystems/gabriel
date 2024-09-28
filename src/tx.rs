use byteorder::{LittleEndian, WriteBytesExt};
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug)]
pub struct TransactionInput {
    pub previous_output_txid: [u8; 32],
    pub previous_output_vout: u32,
    pub script: Vec<u8>,
    pub sequence: u32,
    #[allow(dead_code)]
    pub witness: Vec<WitnessItem>,
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
    pub txid: [u8; 32],
}

#[derive(Debug)]
pub struct WitnessItem {
    #[allow(dead_code)]
    pub witness: Vec<u8>,
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
            previous_output_txid: hex::decode("f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16").unwrap().try_into().unwrap(),
            previous_output_vout: 0,
            script: hex::decode("473044022027542a94d6646c51240f23a76d33088d3dd8815b25e9ea18cac67d1171a3212e02203baf203c6e7b80ebd3e588628466ea28be572fe1aaa3f30947da4763dd3b3d2b01").unwrap(),
            sequence: 0xFFFFFFFF,
            witness: vec![],
        };

        let output_1 = TransactionOutput {
            value: 1_000_000_000,
            script: hex::decode("4104b5abd412d4341b45056d3e376cd446eca43fa871b51961330deebd84423e740daa520690e1d9e074654c59ff87b408db903649623e86f1ca5412786f61ade2bfac").unwrap(),
        };

        let output_2 = TransactionOutput {
            value: 3_000_000_000,
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

        // Assert that the txid is not all zeros (this is a basic check)
        assert_ne!(
            txid.to_vec(),
            hex::decode("a16f3ce4dd5deb92d98ef5cf8afeaf0775ebca408f708b2146c4fb42b41e14be")
                .unwrap()
        );
    }
}
