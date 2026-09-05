# pyfjall

Python bindings for [fjall](https://github.com/fjall-rs/fjall).

```python
from pyfjall import Database

db = Database("./my.db")
items = db.keyspace("items")

items[b"a"] = b"hello"
items.insert(b"b", b"world")
assert items[b"a"] == b"hello"
assert items.get(b"missing") is None

for key, value in items.prefix(b"a"):
    ...
for key in items.range(b"a", b"c", keys_only=True, reverse=True):
    ...

with db.batch() as batch:
    batch.insert(items, b"c", b"1")
    batch.remove(items, b"a")

db.persist("sync_all")
```