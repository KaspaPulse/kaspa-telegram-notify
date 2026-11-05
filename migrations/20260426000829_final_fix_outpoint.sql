-- Direct SQL to fix the constraint issue permanently
ALTER TABLE mined_blocks DROP CONSTRAINT IF EXISTS mined_blocks_outpoint_key;
ALTER TABLE mined_blocks ADD CONSTRAINT mined_blocks_outpoint_key UNIQUE (outpoint);
