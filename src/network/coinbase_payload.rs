//! Strict Kaspa coinbase-payload layout shared by PNN and public REST proof.
//! Blue score 8 bytes || subsidy 8 || script version 2 || script length 1
//! || exact payout script || miner extra data.
pub fn canonical_miner_extra<'a>(payload: &'a [u8], script: &[u8]) -> Option<&'a [u8]> {
    const HEADER_BYTES: usize = 19;
    if script.is_empty() || script.len() > u8::MAX as usize {
        return None;
    }
    if usize::from(*payload.get(18)?) != script.len() {
        return None;
    }
    let script_end = HEADER_BYTES.checked_add(script.len())?;
    if payload.get(HEADER_BYTES..script_end)? != script {
        return None;
    }
    payload.get(script_end..)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_canonical_coinbase_miner_script() {
        let script = [0xab, 0xcd, 0x12];
        let mut payload = vec![0; 18];
        payload.push(3);
        payload.extend(script);
        payload.extend(b"worker");
        assert_eq!(
            canonical_miner_extra(&payload, &script),
            Some(&b"worker"[..])
        );
        assert_eq!(canonical_miner_extra(&payload, &[0xff]), None);
        assert_eq!(canonical_miner_extra(&payload[..18], &script), None);
        assert_eq!(canonical_miner_extra(&payload, &[]), None);
        let mut bad_len = payload.clone();
        bad_len[18] = 2;
        assert_eq!(canonical_miner_extra(&bad_len, &script), None);
    }
    #[test]
    fn a_noncanonical_byte_window_is_not_mining_proof() {
        let mut forged = vec![0; 19];
        forged.extend([0xab, 0xcd, 0x12]);
        forged.extend(b"worker");
        assert_eq!(canonical_miner_extra(&forged, &[0xab, 0xcd, 0x12]), None);
    }
}
