-- Local AI address and model chosen in the app (Settings › Local AI), owner only.
-- NULL = use the server's OLLAMA_URL / OLLAMA_MODEL (docker-compose .env).
ALTER TABLE clinic ADD COLUMN ai_url TEXT, ADD COLUMN ai_model TEXT;
