from .aom import Aom
from .rav1e import Rav1e
from .svtav1 import SvtAv1
from .svtvp9 import SvtVp9
from .vpx import Vpx
from .vvc import Vvc
from .x264 import X264
from .x265 import X265


ENCODERS = {
    'aom': Aom(),
    'rav1e': Rav1e(),
    'svt_av1': SvtAv1(),
    'svt_vp9': SvtVp9(),
    'vpx': Vpx(),
    'vvc': Vvc(),
    'x264': X264(),
    'x265': X265(),
}
