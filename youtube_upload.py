# youtube_upload.py
#
# Upload videos to YouTube using the Data API v3 with OAuth2.
#
# Usage:
#   python youtube_upload.py <video_dir> <youtube_toml> <stem>
#   python youtube_upload.py <video_dir> <youtube_toml> <stem> --auth-only
#   python youtube_upload.py <video_dir> <youtube_toml> <stem> --dry-run
#
# The _youtube.toml file contains metadata templates and upload tracking.
# OAuth tokens are stored at <book_dir>/_yt_token.json (auto-refreshed).
#
# Requires:
#   pip install google-api-python-client google-auth-oauthlib google-auth-httplib2

import argparse
import json
import os
import sys
import tomllib
from pathlib import Path

try:
    import keyring
except ImportError:
    keyring = None


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------
YOUTUBE_API_SERVICE = "youtube"
YOUTUBE_API_VERSION = "v3"
SCOPES = ["https://www.googleapis.com/auth/youtube.upload"]
TOKEN_FILENAME = "_yt_token.json"
VALID_PRIVACY = {"public", "unlisted", "private"}
VALID_CATEGORIES = {
    "1": "Film & Animation", "2": "Autos & Vehicles", "10": "Music",
    "15": "Pets & Animals", "17": "Sports", "18": "Short Movies",
    "19": "Travel & Events", "20": "Gaming", "22": "People & Blogs",
    "23": "Comedy", "24": "Entertainment", "25": "News & Politics",
    "26": "Howto & Style", "27": "Education", "28": "Science & Technology",
    "29": "Nonprofits & Activism",
}


def load_youtube_config(toml_path: Path) -> dict:
    """Load _youtube.toml config."""
    with open(toml_path, "rb") as f:
        return tomllib.load(f)


def save_upload_record(toml_path: Path, stem: str, video_id: str):
    """Append an upload record to the [uploads] section of _youtube.toml."""
    import toml  # toml for writing (tomllib is read-only)
    with open(toml_path, "rb") as f:
        data = tomllib.load(f)
    if "uploads" not in data:
        data["uploads"] = {}
    data["uploads"][stem] = video_id
    with open(toml_path, "w", encoding="utf-8") as f:
        toml.dump(data, f)


def resolve_template(template: str, variables: dict) -> str:
    """Resolve {variable} placeholders in a template string."""
    result = template
    for key, val in variables.items():
        result = result.replace(f"{{{key}}}", str(val))
    return result


def get_client_secret(config: dict) -> dict:
    """Get OAuth client secret from config or keyring.

    The client_secret JSON can be provided as:
    1. A file path in config["auth"]["client_secret_file"]
    2. Stored in OS keyring under service "youtube_client_secret.weavelang"
    """
    # Try config file path first
    auth = config.get("auth", {})
    secret_file = auth.get("client_secret_file", "")
    if secret_file and Path(secret_file).exists():
        with open(secret_file, "r") as f:
            return json.load(f)

    # Try workspace-level path passed via environment variable
    secret_file_env = os.environ.get("YOUTUBE_CLIENT_SECRET_FILE", "")
    if secret_file_env and Path(secret_file_env).exists():
        with open(secret_file_env, "r") as f:
            return json.load(f)

    # Try keyring
    if keyring:
        secret_json = keyring.get_password("youtube_client_secret.weavelang", "youtube_client_secret")
        if secret_json:
            return json.loads(secret_json)

    # Try environment variable
    secret_json = os.environ.get("YOUTUBE_CLIENT_SECRET")
    if secret_json:
        return json.loads(secret_json)

    raise RuntimeError(
        "YouTube OAuth client secret not found.\n"
        "Provide it via one of:\n"
        "  1. config set youtube_client_secret_file <path> (workspace-wide, recommended)\n"
        "  2. auth.client_secret_file in _youtube.toml (per-chapter)\n"
        "  3. set key youtube_client_secret <json> (stored in OS keyring)\n"
        "  4. YOUTUBE_CLIENT_SECRET env var (JSON string)"
    )


def get_authenticated_service(config: dict, token_path: Path):
    """Build an authenticated YouTube API service object.

    Uses stored token if available, otherwise runs the OAuth consent flow.
    """
    from google.oauth2.credentials import Credentials
    from google_auth_oauthlib.flow import InstalledAppFlow
    from googleapiclient.discovery import build

    creds = None

    # Load existing token
    if token_path.exists():
        creds = Credentials.from_authorized_user_file(str(token_path), SCOPES)

    # Refresh or run new flow
    if not creds or not creds.valid:
        if creds and creds.expired and creds.refresh_token:
            from google.auth.transport.requests import Request
            creds.refresh(Request())
        else:
            client_secret = get_client_secret(config)
            flow = InstalledAppFlow.from_client_config(client_secret, SCOPES)
            creds = flow.run_local_server(port=0)

        # Save token for next time
        with open(token_path, "w") as f:
            f.write(creds.to_json())

    return build(YOUTUBE_API_SERVICE, YOUTUBE_API_VERSION, credentials=creds)


def find_video_file(video_dir: Path, stem: str) -> Path:
    """Find the video file for a given stem."""
    for ext in [".mp4", ".mkv", ".webm", ".avi"]:
        candidate = video_dir / f"{stem}{ext}"
        if candidate.exists():
            return candidate
    raise FileNotFoundError(f"No video file found for stem '{stem}' in {video_dir}")


def find_thumbnail(illustrations_dir: Path) -> Path | None:
    """Find the first illustration to use as thumbnail."""
    if not illustrations_dir.exists():
        return None
    for ext in ["*.png", "*.jpg", "*.jpeg"]:
        files = sorted(illustrations_dir.glob(ext))
        if files:
            return files[0]
    return None


def upload_video(
    youtube,
    video_path: Path,
    title: str,
    description: str,
    tags: list[str],
    category_id: str,
    privacy: str,
    language: str,
    thumbnail_path: Path | None = None,
) -> str:
    """Upload a video to YouTube. Returns the video ID."""
    from googleapiclient.http import MediaFileUpload

    body = {
        "snippet": {
            "title": title,
            "description": description,
            "tags": tags,
            "categoryId": category_id,
            "defaultLanguage": language,
        },
        "status": {
            "privacyStatus": privacy,
            "selfDeclaredMadeForKids": False,
        },
    }

    media = MediaFileUpload(
        str(video_path),
        chunksize=10 * 1024 * 1024,  # 10MB chunks
        resumable=True,
    )

    request = youtube.videos().insert(
        part="snippet,status",
        body=body,
        media_body=media,
    )

    print(f"  Uploading: {video_path.name} ({video_path.stat().st_size / 1024 / 1024:.1f} MB)")
    response = None
    while response is None:
        status, response = request.next_chunk()
        if status:
            pct = int(status.progress() * 100)
            print(f"    {pct}% uploaded...", flush=True)

    video_id = response["id"]
    print(f"  Upload complete: https://youtu.be/{video_id}")

    # Set thumbnail if available
    if thumbnail_path and thumbnail_path.exists():
        try:
            youtube.thumbnails().set(
                videoId=video_id,
                media_body=MediaFileUpload(str(thumbnail_path)),
            ).execute()
            print(f"  Thumbnail set: {thumbnail_path.name}")
        except Exception as e:
            print(f"  Warning: Could not set thumbnail: {e}")

    return video_id


def main():
    parser = argparse.ArgumentParser(description="Upload videos to YouTube.")
    parser.add_argument("video_dir", type=Path, help="Directory containing video files")
    parser.add_argument("youtube_toml", type=Path, help="Path to _youtube.toml config")
    parser.add_argument("stem", help="Video stem name to upload")
    parser.add_argument("--book-dir", type=Path, default=None,
                        help="Book directory (for token storage; default: youtube_toml parent)")
    parser.add_argument("--illustrations-dir", type=Path, default=None,
                        help="Illustrations directory (for thumbnail)")
    parser.add_argument("--auth-only", action="store_true",
                        help="Only authenticate (run OAuth flow), don't upload")
    parser.add_argument("--dry-run", action="store_true",
                        help="Show resolved metadata without uploading")
    parser.add_argument("--variables", type=str, default="",
                        help="Extra template variables as key=value,key2=value2")
    args = parser.parse_args()

    # Load config
    if not args.youtube_toml.exists():
        print(f"Error: _youtube.toml not found: {args.youtube_toml}", file=sys.stderr)
        sys.exit(1)
    config = load_youtube_config(args.youtube_toml)

    book_dir = args.book_dir or args.youtube_toml.parent
    token_path = book_dir / TOKEN_FILENAME

    # Auth-only mode
    if args.auth_only:
        print("Running OAuth authentication flow...")
        get_authenticated_service(config, token_path)
        print(f"Token saved to: {token_path}")
        return

    # Build template variables
    template_vars = {
        "stem": args.stem,
        "book_dir": str(book_dir),
    }
    # Auto-extract book_name, chapter_name, level from stem
    import re
    m = re.search(r'_UL([a-z]?)(\d+)$', args.stem)
    if m:
        prefix = args.stem[:m.start()]
        parts = prefix.split('_', 1)
        template_vars["book_name"] = parts[0]
        template_vars["chapter_name"] = parts[1].replace('_', ' ') if len(parts) > 1 else ""
        template_vars["level"] = m.group(2)
        template_vars["level_tag"] = m.group(0)[1:]  # e.g. "UL14", "ULa37"
    # Parse extra variables
    if args.variables:
        for pair in args.variables.split(","):
            if "=" in pair:
                k, v = pair.split("=", 1)
                template_vars[k.strip()] = v.strip()

    # Merge variables from config
    for k, v in config.get("variables", {}).items():
        if k not in template_vars:
            template_vars[k] = v

    # Resolve metadata
    meta = config.get("metadata", {})
    title = resolve_template(meta.get("title_template", "{stem}"), template_vars)
    description = resolve_template(meta.get("description_template", ""), template_vars)
    tags = [resolve_template(t, template_vars) for t in meta.get("tags", [])]
    category_id = str(meta.get("category_id", "27"))  # 27 = Education
    privacy = meta.get("privacy", "unlisted")
    language = meta.get("language", "en")

    if privacy not in VALID_PRIVACY:
        print(f"Error: Invalid privacy '{privacy}'. Must be one of: {VALID_PRIVACY}", file=sys.stderr)
        sys.exit(1)

    # Check upload tracking
    uploads = config.get("uploads", {})
    if args.stem in uploads:
        vid = uploads[args.stem]
        print(f"Already uploaded: {args.stem} → https://youtu.be/{vid}")
        print("Delete the [uploads] entry in _youtube.toml to re-upload.")
        return

    # Find video file
    try:
        video_path = find_video_file(args.video_dir, args.stem)
    except FileNotFoundError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

    # Find thumbnail
    ill_dir = args.illustrations_dir or book_dir / "illustrations"
    thumbnail = find_thumbnail(ill_dir)

    # Dry run
    if args.dry_run:
        print(f"Video:       {video_path}")
        print(f"Title:       {title}")
        print(f"Description: {description[:200]}{'...' if len(description) > 200 else ''}")
        print(f"Tags:        {', '.join(tags)}")
        print(f"Category:    {category_id} ({VALID_CATEGORIES.get(category_id, 'Unknown')})")
        print(f"Privacy:     {privacy}")
        print(f"Language:    {language}")
        print(f"Thumbnail:   {thumbnail or '(none)'}")
        print("\nDry run complete.")
        return

    # Authenticate and upload
    youtube = get_authenticated_service(config, token_path)
    video_id = upload_video(
        youtube, video_path, title, description, tags,
        category_id, privacy, language, thumbnail,
    )

    # Record successful upload
    try:
        save_upload_record(args.youtube_toml, args.stem, video_id)
        print(f"  Recorded upload in _youtube.toml: {args.stem} = {video_id}")
    except Exception as e:
        print(f"  Warning: Could not record upload: {e}")
        print(f"  Manually add to [uploads]: {args.stem} = \"{video_id}\"")


if __name__ == "__main__":
    main()
