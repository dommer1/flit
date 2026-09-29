-- Every word the search index holds, with how many messages use it — what a
-- mistyped search word is compared against to find the word it meant.
-- fts5vocab stores nothing of its own: it reads messages_fts's index, so it
-- is always current and costs no space.
CREATE VIRTUAL TABLE messages_fts_vocab USING fts5vocab(messages_fts, 'row');
