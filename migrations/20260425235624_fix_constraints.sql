ALTER TABLE user_wallets DROP CONSTRAINT IF EXISTS user_wallets_pkey CASCADE;
ALTER TABLE user_wallets ADD PRIMARY KEY (wallet, chat_id);

ALTER TABLE mined_blocks DROP CONSTRAINT IF EXISTS mined_blocks_pkey CASCADE;
ALTER TABLE mined_blocks ADD PRIMARY KEY (wallet, outpoint);

ALTER TABLE knowledge_base DROP CONSTRAINT IF EXISTS knowledge_base_link_key CASCADE;
ALTER TABLE knowledge_base ADD CONSTRAINT knowledge_base_link_key UNIQUE (link);
