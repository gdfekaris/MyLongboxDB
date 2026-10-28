-- Deleting a book's last copy for good used to leave its edition and
-- issue behind, with their credits and cast, though no book of them was
-- owned any more; so did taking an issue off a collected edition's
-- contents. The collection holds the books owned, so these go: editions
-- outside Trash with no copy, then issues outside Trash with no edition
-- that no collected edition reprints. Series stay.

DELETE FROM edition WHERE trash_id IS NULL
    AND NOT EXISTS (SELECT 1 FROM copy WHERE copy.edition_id = edition.id);

DELETE FROM issue WHERE trash_id IS NULL
    AND NOT EXISTS (SELECT 1 FROM edition WHERE edition.issue_id = issue.id)
    AND NOT EXISTS (SELECT 1 FROM collects WHERE collects.collected_issue_id = issue.id);

-- Again: a collected edition deleted above may have been all that
-- reprinted an issue. Collected editions reprint only single issues,
-- so a third pass would find nothing.
DELETE FROM issue WHERE trash_id IS NULL
    AND NOT EXISTS (SELECT 1 FROM edition WHERE edition.issue_id = issue.id)
    AND NOT EXISTS (SELECT 1 FROM collects WHERE collects.collected_issue_id = issue.id);
