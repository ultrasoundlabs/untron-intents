use super::*;

impl SolverDb {
    pub async fn save_tron_proof(&self, txid: [u8; 32], proof: &TronProofRow) -> Result<()> {
        sqlx::query(
            "insert into solver.tron_proofs(txid, blocks, encoded_tx, proof, index_dec) \
             values ($1, $2, $3, $4, $5) \
             on conflict (txid) do update set \
               blocks = excluded.blocks, \
               encoded_tx = excluded.encoded_tx, \
               proof = excluded.proof, \
               index_dec = excluded.index_dec",
        )
        .bind(txid.to_vec())
        .bind(&proof.blocks)
        .bind(&proof.encoded_tx)
        .bind(&proof.proof)
        .bind(&proof.index_dec)
        .execute(&self.pool)
        .await
        .context("save solver.tron_proofs")?;
        Ok(())
    }

    pub async fn load_tron_proof(&self, txid: [u8; 32]) -> Result<TronProofRow> {
        let row = sqlx::query(
            "select blocks, encoded_tx, proof, index_dec from solver.tron_proofs where txid = $1",
        )
        .bind(txid.to_vec())
        .fetch_one(&self.pool)
        .await
        .context("load solver.tron_proofs")?;
        Ok(TronProofRow {
            blocks: row.try_get("blocks")?,
            encoded_tx: row.try_get("encoded_tx")?,
            proof: row.try_get("proof")?,
            index_dec: row.try_get("index_dec")?,
        })
    }
}
