-- 1. Table for tracking user wallets
CREATE TABLE IF NOT EXISTS user_wallets (
    wallet TEXT NOT NULL,
    chat_id BIGINT NOT NULL,
    PRIMARY KEY (wallet, chat_id)
);

-- 2. Table for tracking mined blocks and rewards
CREATE TABLE IF NOT EXISTS mined_blocks (
    wallet TEXT NOT NULL,
    outpoint TEXT NOT NULL,
    amount BIGINT NOT NULL,
    daa_score BIGINT NOT NULL,
    timestamp TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (wallet, outpoint)
);

-- 3. Table for AI Chat History
CREATE TABLE IF NOT EXISTS chat_history (
    id SERIAL PRIMARY KEY,
    chat_id BIGINT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

-- 4. Table for the AI Knowledge Base (RAG System)
CREATE TABLE IF NOT EXISTS knowledge_base (
    id SERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    link TEXT UNIQUE NOT NULL,
    content TEXT NOT NULL,
    source TEXT NOT NULL,
    embedding JSONB,
    published_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);
