from argostranslate import sbd as _gui_argos_sbd
import minisbd.models as _gui_sbd
import shutil as _gui_shutil
_gui_old_caches = {_gui_sbd.cache_dir, os.path.join(_gui_sbd.get_user_cache_dir(), 'minisbd')}
_gui_sbd.cache_dir = os.path.join(os.environ['XDG_CACHE_HOME'], 'minisbd')
os.makedirs(_gui_sbd.cache_dir, exist_ok=True)
# Copy only selected segmentation models; leave the previous cache intact.
_gui_codes = None
if '--load-only' in sys.argv:
    _gui_codes = set(sys.argv[sys.argv.index('--load-only') + 1].split(','))
elif len(sys.argv) == 2 and ',' in sys.argv[1]:
    _gui_codes = set(sys.argv[1].split(','))
if _gui_codes is not None:
    _gui_mapping = _gui_argos_sbd.MiniSBDSentencizer.LANGUAGE_CODE_MAPPING
    _gui_codes = {_gui_mapping.get(code, code) for code in _gui_codes}
    _gui_codes = {code if code in _gui_sbd.MODELS else 'en' for code in _gui_codes}
for _gui_code, _gui_filename in _gui_sbd.MODELS.items():
    if _gui_codes is not None and _gui_code not in _gui_codes:
        continue
    _gui_target = os.path.join(_gui_sbd.cache_dir, _gui_filename)
    for _gui_old_cache in _gui_old_caches:
        _gui_source = os.path.join(_gui_old_cache, _gui_filename)
        if os.path.isfile(_gui_source) and not os.path.exists(_gui_target):
            _gui_shutil.copy2(_gui_source, _gui_target)

_gui_key = os.environ.pop('LIBRETRANSLATE_GUI_API_KEY', '')
if _gui_key:
    from libretranslate.api_keys import Database
    _gui_db_path = os.environ.pop('LIBRETRANSLATE_GUI_API_DB')
    _gui_db = Database(_gui_db_path)
    _gui_previous_path = _gui_db_path + '.managed-key'
    if os.path.isfile(_gui_previous_path):
        with open(_gui_previous_path, encoding='utf-8') as _gui_file:
            _gui_previous = _gui_file.read()
        if _gui_previous != _gui_key:
            _gui_db.remove(_gui_previous)
    _gui_limit = int(os.environ.pop('LIBRETRANSLATE_GUI_KEY_LIMIT', '-1'))
    # Upstream converts its unlimited CLI sentinel before reading key quotas.
    # The key database itself needs a positive quota, including API-key-only mode.
    _gui_db.add(_gui_limit if _gui_limit > 0 else 9999999999999, _gui_key)
    with open(_gui_previous_path, 'w', encoding='utf-8') as _gui_file:
        _gui_file.write(_gui_key)
    _gui_db.c.close()
    del _gui_key
