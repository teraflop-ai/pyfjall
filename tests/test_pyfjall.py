import pytest

from pyfjall import Database, Error


@pytest.fixture
def ks(tmp_path):
    return Database(tmp_path / "db").keyspace("items")


def test_crud(ks):
    ks.insert(b"a", b"1")
    ks[b"b"] = b"2"
    assert ks.get(b"a") == b"1"
    assert ks[b"b"] == b"2"
    assert b"a" in ks
    assert ks.get(b"zzz") is None
    with pytest.raises(KeyError):
        ks[b"zzz"]
    assert ks.size_of(b"a") == 1
    del ks[b"a"]
    assert b"a" not in ks
    assert ks.len() == 1
    ks.clear()
    assert ks.is_empty()


def test_iteration(ks):
    for k in b"abcd":
        ks.insert(bytes([k]), bytes([k]).upper())
    assert list(ks.iter()) == [(b"a", b"A"), (b"b", b"B"), (b"c", b"C"), (b"d", b"D")]
    assert list(ks.iter(reverse=True, keys_only=True)) == [b"d", b"c", b"b", b"a"]
    assert list(ks.prefix(b"b")) == [(b"b", b"B")]
    assert list(ks.range(b"b", b"d", keys_only=True)) == [b"b", b"c"]
    assert list(ks.range(end=b"b", keys_only=True)) == [b"a"]
    assert ks.first_key_value() == (b"a", b"A")
    assert ks.last_key_value() == (b"d", b"D")


def test_batch(tmp_path):
    db = Database(tmp_path / "db")
    a, b = db.keyspace("a"), db.keyspace("b")
    with db.batch() as batch:
        batch.insert(a, b"k", b"1")
        batch.insert(b, b"k", b"2")
        assert len(batch) == 2
        assert a.get(b"k") is None
    assert (a[b"k"], b[b"k"]) == (b"1", b"2")
    with pytest.raises(Error):
        batch.commit()


def test_reopen(tmp_path):
    with Database(tmp_path / "db") as db:
        db.keyspace("items").insert(b"k", b"v")
    del db
    db = Database(tmp_path / "db")
    assert db.keyspace_exists("items")
    assert db.keyspace("items")[b"k"] == b"v"


def test_kv_separation(tmp_path):
    ks = Database(tmp_path / "db").keyspace("blobs", kv_separation=True)
    big = b"x" * (1 << 20)
    ks.insert(b"big", big)
    assert ks.get(b"big") == big
    assert next(ks.iter(keys_only=True)) == b"big"


def test_persist_mode(tmp_path):
    db = Database(tmp_path / "db", cache_size=1 << 20)
    db.persist("sync_data")
    with pytest.raises(ValueError):
        db.persist("nope")
