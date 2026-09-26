#!/usr/bin/env python3
"""A stand-in for GitHub's REST API as crates/oracle-web calls it, for the dev lane.

lanes/dev.sh runs it in the dev pod when .tmp/lane-dev/fake-github/releases exists and points
oracle-web at it (GITHUB_API and a dummy GITHUB_TOKEN), so that the lane offers test releases
without GitHub. It answers the calls oracle-web makes under /repos/<owner>/<name>/, whichever
repository they name:

  GET  releases?per_page=&page=  the releases in <dir>/releases, newest first, a page at a time,
                                 with a weak ETag; an If-None-Match naming it gets a bare 304
  GET  releases/assets/<id>      a file's JSON; with Accept: application/octet-stream, a 302 to
                                 /storage/<id>/<name>, which sends its bytes and, like GitHub's
                                 storage, needs no token
  GET  labels                    the labels: GitHub's defaults, and those created since
  POST labels                    a new label; 422 already_exists for one it has
  POST issues                    an issue, numbered, written to <dir>/issues/<number>.json and
                                 logged: a report sent in the lane goes nowhere else

A release is a folder <dir>/releases/<tag>/ holding its files and a release.json of
{"draft": bool, "prerelease": bool, "created_at": "<ISO 8601>"}, each field optional;
lanes/publish-release.sh lays releases out. A folder or file whose name starts with "." is
unfinished and never listed. An asset's id comes from its name, size and modification time, so a
replaced file is another asset, as a new upload is on GitHub. Every call but /storage needs the
token (Authorization: Bearer <token>) and is refused with 401 otherwise.

  python3 lanes/fake-github.py --dir .tmp/lane-dev/fake-github --token <token> [--port 3001]
"""

import argparse
import hashlib
import json
import os
import re
import threading
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, quote, unquote, urlsplit

# The labels GitHub gives a new repository.
DEFAULT_LABELS = {
    "bug": ("d73a4a", "Something isn't working"),
    "documentation": ("0075ca", "Improvements or additions to documentation"),
    "duplicate": ("cfd3d7", "This issue or pull request already exists"),
    "enhancement": ("a2eeef", "New feature or request"),
    "good first issue": ("7057ff", "Good for newcomers"),
    "help wanted": ("008672", "Extra attention is needed"),
    "invalid": ("e4e669", "This doesn't seem right"),
    "question": ("d876e3", "Further information is requested"),
    "wontfix": ("ffffff", "This will not be worked on"),
}
# Where issue and release pages would be: a reserved name (RFC 2606), which leads nowhere.
PAGES = "http://fake-github.invalid"
DOCS = "https://docs.github.com/rest"
REPO_CALL = re.compile(r"^/repos/([^/]+)/([^/]+)/(.+)$")
STORAGE = re.compile(r"^/storage/(\d+)/([^/]+)$")
# GitHub's page size when none is asked for, and its most.
PER_PAGE = 30
MAX_PER_PAGE = 100
CHUNK = 64 * 1024


def log(message):
    print(f"fake-github: {message}", flush=True)


def number_of(text):
    """A stable 48-bit number for `text`: release and asset ids."""
    return int.from_bytes(hashlib.sha256(text.encode()).digest()[:6], "big")


def iso(timestamp):
    return datetime.fromtimestamp(timestamp, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def names(if_none_match, etag):
    """Whether If-None-Match names `etag`, compared weakly as RFC 9110 has it."""
    etag = etag.removeprefix("W/")
    return any(
        tag.strip() == "*" or tag.strip().removeprefix("W/") == etag
        for tag in if_none_match.split(",")
    )


def number(query, name, default, least, most):
    """The query parameter `name` as a number from `least` to `most` (None: no bound)."""
    try:
        value = int(query.get(name, [default])[0])
    except ValueError:
        value = default
    value = max(value, least)
    return value if most is None else min(value, most)


class Store:
    """The releases on disk, and the labels and issues the reports make."""

    def __init__(self, root):
        self.root = root
        self.lock = threading.Lock()
        self.labels = {
            name: {"name": name, "color": color, "description": description}
            for name, (color, description) in DEFAULT_LABELS.items()
        }

    def releases(self, repo="-/-"):
        """Every release as GitHub lists it, newest first, and each file's path by asset id.
        `repo`, owner/name, only goes into the pages' addresses."""
        listed, files = [], {}
        folder = self.root / "releases"
        entries = sorted(folder.iterdir()) if folder.is_dir() else []
        for entry in entries:
            if entry.name.startswith(".") or not entry.is_dir():
                continue
            try:
                listed.append(self.release(entry, repo, files))
            except FileNotFoundError:
                # Replaced by lanes/publish-release.sh while this listing read it.
                continue
            except (OSError, ValueError) as error:
                log(f"{entry.name}: {error}; the release is left out")
        listed.sort(key=lambda release: (release["created_at"], release["tag_name"]), reverse=True)
        return listed, files

    def release(self, folder, repo, files):
        """The release in `folder`, as GitHub's JSON has it; its files go into `files`."""
        tag = folder.name
        meta_file = folder / "release.json"
        meta = json.loads(meta_file.read_text()) if meta_file.exists() else {}
        draft = bool(meta.get("draft", False))
        created = meta.get("created_at") or iso(folder.stat().st_mtime)
        assets = []
        for path in sorted(folder.iterdir()):
            if path.name.startswith(".") or path.name == "release.json" or not path.is_file():
                continue
            stat = path.stat()
            asset_id = number_of(f"{tag}/{path.name}/{stat.st_size}/{stat.st_mtime_ns}/{stat.st_ino}")
            files[asset_id] = path
            assets.append(
                {
                    "id": asset_id,
                    "name": path.name,
                    "label": "",
                    "content_type": "application/octet-stream",
                    "state": "uploaded",
                    "size": stat.st_size,
                    "created_at": iso(stat.st_mtime),
                    "browser_download_url": (
                        f"{PAGES}/{repo}/releases/download/{quote(tag)}/{quote(path.name)}"
                    ),
                }
            )
        return {
            "id": number_of(f"release/{tag}"),
            "tag_name": tag,
            "name": meta.get("name", tag),
            "draft": draft,
            "prerelease": bool(meta.get("prerelease", False)),
            "created_at": created,
            "published_at": None if draft else meta.get("published_at", created),
            "html_url": f"{PAGES}/{repo}/releases/tag/{quote(tag)}",
            "assets": assets,
        }

    def asset(self, asset_id, repo):
        """The asset `asset_id`, of any release, drafts included, and its file's path."""
        listed, files = self.releases(repo)
        for release in listed:
            for asset in release["assets"]:
                if asset["id"] == asset_id:
                    return asset, files[asset_id]
        return None, None

    def create_label(self, label):
        with self.lock:
            if label["name"] in self.labels:
                return False
            self.labels[label["name"]] = label
            return True

    def open_issue(self, repo, title, body, labels):
        folder = self.root / "issues"
        with self.lock:
            folder.mkdir(parents=True, exist_ok=True)
            taken = [int(path.stem) for path in folder.glob("*.json") if path.stem.isdigit()]
            number = max(taken, default=0) + 1
            issue = {
                "number": number,
                "html_url": f"{PAGES}/{repo}/issues/{number}",
                "title": title,
                "body": body,
                "labels": [{"name": name} for name in labels],
                "state": "open",
                "created_at": iso(datetime.now(timezone.utc).timestamp()),
            }
            path = folder / f"{number}.json"
            path.write_text(json.dumps(issue, ensure_ascii=False, indent=2) + "\n")
        return issue, path


class Handler(BaseHTTPRequestHandler):
    # Keeps connections open as GitHub does: oracle-web's client reuses them.
    protocol_version = "HTTP/1.1"
    server_version = "fake-github"
    store = None
    token = None

    def do_GET(self):
        self.body = b""
        self.route("GET")

    def do_POST(self):
        # Read whatever the answer: left unread, it would be taken for the connection's next call.
        self.body = self.read_body()
        self.route("POST")

    def log_message(self, format, *args):
        """Quiet: `route` logs what matters."""

    def route(self, method):
        url = urlsplit(self.path)
        storage = STORAGE.match(url.path)
        if method == "GET" and storage:
            return self.send_file(int(storage[1]), unquote(storage[2]))
        if self.headers.get("Authorization", "") not in (
            f"Bearer {self.token}",
            f"token {self.token}",
        ):
            self.send_json(401, {"message": "Bad credentials", "documentation_url": DOCS})
            return log(f"{method} {url.path}: 401, no or another token")
        call = REPO_CALL.match(url.path)
        if not call:
            return self.not_found(method, url.path)
        owner, name, rest = call.groups()
        repo = f"{owner}/{name}"
        query = parse_qs(url.query)
        if method == "GET" and rest == "releases":
            return self.list_releases(repo, query)
        if method == "GET" and rest.startswith("releases/assets/"):
            asset_id = rest.removeprefix("releases/assets/")
            if asset_id.isdigit():
                return self.send_asset(repo, int(asset_id))
        if method == "GET" and rest == "labels":
            return self.list_labels(query)
        if method == "POST" and rest == "labels":
            return self.create_label()
        if method == "POST" and rest == "issues":
            return self.open_issue(repo)
        return self.not_found(method, url.path)

    def list_releases(self, repo, query):
        per_page = number(query, "per_page", PER_PAGE, 1, MAX_PER_PAGE)
        page = number(query, "page", 1, 1, None)
        listed, _ = self.store.releases(repo)
        shown = listed[(page - 1) * per_page : page * per_page]
        body = json.dumps(shown).encode()
        etag = f'W/"{hashlib.sha256(body).hexdigest()[:32]}"'
        if names(self.headers.get("If-None-Match", ""), etag):
            self.send_response(304)
            self.send_header("ETag", etag)
            self.end_headers()
            return
        self.send_body(200, body, "application/json; charset=utf-8", [("ETag", etag)])
        tags = ", ".join(
            release["tag_name"] + (" (draft)" if release["draft"] else "") for release in shown
        )
        log(f"listed page {page}: {tags or 'no releases'}")

    def send_asset(self, repo, asset_id):
        asset, path = self.store.asset(asset_id, repo)
        if asset is None:
            return self.not_found("GET", f"releases/assets/{asset_id}")
        if self.headers.get("Accept", "") != "application/octet-stream":
            return self.send_json(200, asset)
        # GitHub sends the file from its storage, on another host; here, the same one.
        host = self.headers.get("Host", f"127.0.0.1:{self.server.server_address[1]}")
        location = f"http://{host}/storage/{asset_id}/{quote(asset['name'])}"
        self.send_body(302, b"", "text/plain", [("Location", location)])

    def send_file(self, asset_id, name):
        _, files = self.store.releases()
        path = files.get(asset_id)
        try:
            if path is None or path.name != name:
                raise FileNotFoundError
            file = path.open("rb")
        except FileNotFoundError:
            return self.not_found("GET", f"/storage/{asset_id}/{name}")
        with file:
            size = os.fstat(file.fileno()).st_size
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(size))
            self.end_headers()
            while chunk := file.read(CHUNK):
                self.wfile.write(chunk)
        log(f"sent {path.parent.name}/{name}, {size} bytes")

    def list_labels(self, query):
        per_page = number(query, "per_page", PER_PAGE, 1, MAX_PER_PAGE)
        with self.store.lock:
            labels = list(self.store.labels.values())[:per_page]
        self.send_json(200, labels)

    def create_label(self):
        label = self.read_json()
        if not isinstance(label, dict) or not label.get("name"):
            return self.send_json(422, {"message": "Validation Failed", "documentation_url": DOCS})
        label = {
            "name": label["name"],
            "color": label.get("color", "ededed"),
            "description": label.get("description", ""),
        }
        if not self.store.create_label(label):
            return self.send_json(
                422,
                {
                    "message": "Validation Failed",
                    "errors": [{"resource": "Label", "code": "already_exists", "field": "name"}],
                    "documentation_url": DOCS,
                },
            )
        self.send_json(201, label)
        log(f"created the label {label['name']!r}")

    def open_issue(self, repo):
        issue = self.read_json()
        if not isinstance(issue, dict) or not issue.get("title"):
            return self.send_json(422, {"message": "Validation Failed", "documentation_url": DOCS})
        labels = [str(label) for label in issue.get("labels") or []]
        opened, path = self.store.open_issue(repo, issue["title"], issue.get("body", ""), labels)
        self.send_json(201, opened)
        log(f"issue #{opened['number']} {issue['title']!r}, labels {labels}: {path}")

    def not_found(self, method, path):
        self.send_json(404, {"message": "Not Found", "documentation_url": DOCS})
        log(f"{method} {path}: 404")

    def read_body(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            body = b""
            while size := int(self.rfile.readline().split(b";")[0].strip(), 16):
                body += self.rfile.read(size)
                self.rfile.readline()
            self.rfile.readline()
            return body
        return self.rfile.read(int(self.headers.get("Content-Length") or 0))

    def read_json(self):
        try:
            return json.loads(self.body)
        except ValueError:
            return None

    def send_json(self, status, value):
        body = json.dumps(value, ensure_ascii=False).encode()
        self.send_body(status, body, "application/json; charset=utf-8", [])

    def send_body(self, status, body, content_type, headers):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        for name, value in headers:
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser(description="A stand-in for GitHub's API, for oracle-web.")
    parser.add_argument("--dir", type=Path, required=True, help="releases/ and issues/ live here")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=3001)
    parser.add_argument(
        "--token",
        default=os.environ.get("FAKE_GITHUB_TOKEN"),
        help="the token calls must carry (default: $FAKE_GITHUB_TOKEN)",
    )
    args = parser.parse_args()
    if not args.token:
        parser.error("no token: pass --token or set FAKE_GITHUB_TOKEN")
    Handler.store = Store(args.dir)
    Handler.token = args.token
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    server.daemon_threads = True
    listed, _ = Handler.store.releases()
    tags = ", ".join(
        release["tag_name"] + (" (draft)" if release["draft"] else "") for release in listed
    )
    log(f"serving {args.dir} on http://{args.host}:{args.port}: {tags or 'no releases yet'}")
    server.serve_forever()


if __name__ == "__main__":
    main()
