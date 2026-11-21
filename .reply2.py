import json
import subprocess

raw = subprocess.run(
    ['gh', 'api', 'repos/storytold/craft-libs/pulls/2845/comments', '--jq', '.[] | select(.user.login == "coderabbitai[bot]")'],
    capture_output=True,
    text=True,
).stdout
items = json.loads(raw)

bodies = {
    'write.rs:175': "Fixed in 69bfe6ab3: the fill's own `svg` is resolved first and the derived one only stands in when that relationship is unavailable, so an explicit vector is never replaced.",
    'write.rs:238': "Fixed in 69bfe6ab3: `Exp::content_type` now holds the rule `media_part` used (the item's own type, or the one its extension means), and `is_svg` asks it, so an SVG part with no content type still gets its PNG fallback. Linked items stay excluded: the check still requires bytes. Test: `an_item_without_a_content_type_is_still_a_vector`.",
}


def key(comment):
    line = comment.get('line') or comment.get('original_line') or 0
    if comment['path'].endswith('drawingml/src/write.rs'):
        return 'write.rs:175'
    return 'write.rs:238' if line > 200 else 'write.rs:175'


for comment in items:
    subprocess.run(
        [
            'gh', 'api', '--method', 'POST', 'repos/storytold/craft-libs/pulls/2845/comments',
            '-f', f"body={bodies[key(comment)]}",
            '-F', f"in_reply_to={comment['id']}",
            '--jq', '.id',
        ],
        capture_output=True,
        text=True,
    )
print('replied to', len(items))
