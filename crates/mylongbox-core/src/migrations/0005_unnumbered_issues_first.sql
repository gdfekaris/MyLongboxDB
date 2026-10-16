-- An issue numbered "nn", published without a number, now sorts before
-- the numbered issues of its series rather than after them. Its stored
-- sort key is the number's normalized text, so "nn", "NN", and "#nn"
-- all have the key '2nn' and all move.
UPDATE issue SET number_sort = '!nn' WHERE number_sort = '2nn';
