PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE embeddings (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	reference TEXT,
	text TEXT,
	embedding BLOB 
);
INSERT INTO embeddings VALUES(1,'My ref','My text',X'5b312e323330303030303139303733343836332c342e3535393939393934323737393534312c372e3838393939393836363438353539365d');
DELETE FROM sqlite_sequence;
INSERT INTO sqlite_sequence VALUES('embeddings',1);
COMMIT;