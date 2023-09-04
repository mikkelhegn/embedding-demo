CREATE TABLE IF NOT EXISTS embeddings (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	reference TEXT UNIQUE,
	text TEXT NOT NULL,
	embedding BLOB
);

-- DROP TABLE IF EXISTS vss_docs_content;

-- CREATE virtual TABLE vss_docs_content USING vss0(embedding(384));

-- insert into vss_docs_content(embedding) select embedding from docs_content;
