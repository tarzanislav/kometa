"""Grafické okno: karty Sťahovanie, Jazykové stopy (YouTube) a Účty a nastavenia."""

from __future__ import annotations

import os
import queue
import re
import shutil
import threading
import tkinter as tk
from pathlib import Path
from tkinter import filedialog, messagebox, ttk
from tkinter import font as tkfont

from . import media, skener
from .common import (ANSI, APP_NAME, CONFIG_DIR, DEFAULT_LANGS, Cancelled, apply_settings, check_dependencies,
                     clean_error, default_outdir, delete_password, format_duration, get_password, human_eta,
                     human_size, human_speed, load_config, open_folder, safe_name, save_config, set_password)
from .fronta import STATUS_TEXT, DownloadQueue
from .hosts import Accounts, FastshareClient, WebshareClient
from .jazyky import fetch_info, list_audio_tracks, pick_tracks, run_job

BG, CARD, BORDER = "#f3f4f6", "#ffffff", "#e2e5ea"
TEXT, MUTED = "#1f2328", "#6b7280"
ACCENT, ACCENT_DARK, ACCENT_SOFT = "#e0312b", "#bf2620", "#fde8e7"
QUALITIES = ["Najlepšia", "2160p", "1440p", "1080p", "720p", "480p"]

# Obrázky zaškrtávacích políčok a prepínačov (PNG, base64)
_ICONS = {
    "check_off_16": "iVBORw0KGgoAAAANSUhEUgAAABcAAAAQCAYAAAD9L+QYAAABuUlEQVR42tWUT2sTURTFz3nTZEZj1Yh14R+sdqGC/QB1IYKIortKoyj4BcTPMOQzuJZuhLZByaZZuVARRNCVLgQpIiJoqdYmTp1JZt47LlTQhrxowYUH7ubB/b3Dvec9AiJAAcD83fvHVXJ70ySTIYlf5AIpCkPIKrk6feYJAMVxbOr1usMAMY5jMzV1due6y2/uru64GIVhKAjEb2z8PCusdZ/byeO017s+c/7kc/hEAAvNe3PvltuS5P6g7GonVbP1cGlx8VHVxza3brcmx8Z2zezbs70AAEkcUqY6GhVHj0xMrKXJFR98BOSB0Uol+M4VN4y6T5IAgJWtkWjMIa9zULl1Dn8rSZRD7oUbcqjbgfvauPU+5/9Q/zHcBEabbZagYc4Dkpu5QMYw8Oa8sEWnl+cEYAEEP3LszTlJWevopI7X+bZg/NmH5ZWna1+ygGRO0vrKGFOk3Xzk5avX6+VSeX7Y14JG68FkJYwWDo/vP7YlKmNQ7iXBWoc3b99/+riyeuPS9Ok571jiODa1C6dezM42TyRf08skD/aybt8DcXAolUIZsp1l3TvXaueWGo1GUKvV7CD4N3MI/SUEpuExAAAAAElFTkSuQmCC",
    "check_on_16": "iVBORw0KGgoAAAANSUhEUgAAABcAAAAQCAYAAAD9L+QYAAACnElEQVR42o2Uy0tVURTGf2vvrddnvq4ZgZhEg6RbQQ+ofyMqqaBCo0EvCCLpgTkoiIIGQQ0qiiY9aBQ0blZRglERBDlqoFbevOpJO+fsvRqYoaWnPlizvb71rb3Wt0RBBBRgaOPaNWXql0de1KsKc2BFNGeBwOSygXcvAO0F0weBRSC9YLo6OupNpVw1KtsQcjOlFoZCKBOe/Qgcah1494YMiACD6zvutebKOz/FyXy5f8IYUNW8teZbkgyqt5va3r79tujzj4VCwYlsH4qTVACZKfh3iIhGkRBF5kscp9XOrlTjd2VqUautTsR6sAILC3eOMDZGU88ZGk/3QvxDVETFSHsmuUAyr21rmVfDOUJxlIotW1mydz9LOndhamrAB9G5uQuSz9mKMDFBKJVgdqLWoRMTVGzcTMv1m4TSGJ+PHyVEEeIsomSPaHasGsfUHz5G8+Ur4FPEWvR7hG1upuX6DTDCyMEuJp88xlRWQQj8C846ByFFqqqoP3AQqagk+fCB4pVLlK1op+XaDUxDI8O7dzLd/xLXvJQQx+Dsv8l9mlLmDDo1xUj3PhpO9NBwogeTz5Nbu57yNQVGz51h6uVzTGMTmiT8L5yKKKpIeTlT/a+Y3rOTfN956vZ3AzDad5bS3dvYpjyk6XxDSZbdwBkJFoyiiqmtRZOEr6dO4otF0pFhSnduYfNLwad/OlVFNfNvnKiMqyACHu+tWAtV1RQvXkCcnVG8EDGIioxnbktbrP2xhldNztmgmmgIXlFv6uo81TVevfeq+juCalptrYt8iAx6P/O2AAxuKBRq0AdOzGpl8eXVXwlBGZ30/kj76/f3Mslnz+bTdW31q11tJ0jbdAh/GSQAOYtapDQewqNVA+8/PgS7A/xi5D8BXCEqnRGBmt8AAAAASUVORK5CYII=",
    "radio_off_16": "iVBORw0KGgoAAAANSUhEUgAAABcAAAAQCAYAAAD9L+QYAAACW0lEQVR42pWUzWsTURTF731pWpKmia3VlIpacNEuRHEjdiEU7cadH1Bx0b/AnVBwo0P/goK7ggsXmuoQW6ItGCeCqP1wocu2itaqbQOpBmNnJpN03jsukoFQy0x7YGB4M+93Lu/dc5kapOt6aGhoSBIRpadnj5vlUm+0OQrlIn/96sAiMysATETEzKAAsfcCgJkZqafZC5FIdCTeFu1vbY0kmJnKZcf5u2UtWbZz78aVwQeN/wcZkK7rIWamiUljbP7DEkqmAyklAKj6A9OuYPnrOtLTr7Mv5uY6mGsGgWAiokfp7NiXHwVIKV0ALgCllIJSyjORAKqFogn92asFXdebvb2+mpjKnV/4uAQAVQ+6Uw1GlW9rm5iYNG4HcYWmaUIIvnuy7wQRkaif5f+Xw0z19XDPkU4Va4veSqUySV94T9+5Y+2JWH8kEgYRid3AjfLO+Wh31yHV1HLRFy6YehPxeFTQHi6owaP9QBwcEqd94YASHFTubj1caxfhC1eS1mzbruyXbVllBmjVF37KSi7/Kv757FRdqicwqGIQEX9f33Ck6xqBZTxMvxz+tLIBANVay2FXydqH6mat1+/vKZ0AxJOpnFEomgBQaQzQjiBVTbuCTPZdIZXJJTVNE4GtBYAN4/3ByZk3sys/N7HtStQT6aVVulKi8HsLM8Z8Xn+eO1vfKwIHlzeExsfHwx3dvSOxWORm1+HO7vZEnJiJLLtMG/mCVdoyH9tO+c7wtUt5TYMYHWW1r6lIRJTK5JJNHB7Ydp0z4VALiHm1orbfDl8eXPQqZvYHExH9A5TbyLuaumk7AAAAAElFTkSuQmCC",
    "radio_on_16": "iVBORw0KGgoAAAANSUhEUgAAABcAAAAQCAYAAAD9L+QYAAAC4UlEQVR42o2UT2hcVRTGf+fc916bzDTVJDOhVq1SSjQGyrRIaUUR3YkQCrbgQijFnZtQWigFA+66UArdC+JK7SJUrAtdiRuV+q/CWARlFKVxpg2mmUkm7717joukkoXO+MHZnfu797t85wjb5CdOBLlyJQIsNxr7VjWfriAuorcuX7/RfAPMQQAEnCGSf8AgAt46NPPcDvScuR8Vkd0CRPe+ivxoyOUHv/nhne39A+ECvA/hJMTfGjOXRkKYV6AXjYg4gOKyU5WdKqyW9ontsJf3fdFctiEXiEMQiL80Zi7tSdP5pbyIiCAhKEWx6SxN3WN03ONklqa3i/LL3oY/02w240mI/wVXgfjrocefHtMw3y6KAhEVCLayIlKpIJUKtrIiAiqq6e28zGtJcqSScWYQGEB90/WCiGCICogXORMXFti7eI29i9eYuLCAFzm4g5DejdFU5MzPs7NTA+Gtg9MPgxztxegSglp3lfGz5xk7dZpQrxPqdcZOnWb87Hmsu4qEIBvmjKjWNLHnB7/ck+lUZDSK4HkuoVanOncczMDiVhnVueOEWh3PcxBcBVc4OBAuqirbIvl/tRl21YHwEPm9dDbUHckyYqdN9+oiqIKGrVK6VxeJnTaSZbgj5oibtQbBk4ey7GYrrv80GsLsWoym1V26/OZFcKi88CIAvY8/Yvmti2h1F8ToQZB1s350+3Sou1bjiVfGk/DunaIsRDXFHet2CbUaALHTQatVEMHMisk0Se+U5duPftt8dRA8fADh2FLnxp9Tk09NZOmBtdJyBNWREaHfh6JARkfB3d29rCYh7Zl1ykJfur/dXv9syIQKwB9PPjYuMflwdxKO/VVGCnNDNg+6I0HQ+5LAWrSltWhz+79vfuWbQ2gDv+XeErp++HC6x/vnBHktEXngXozMIXfrOfLeXSlfn/n65q1h4H/digBLR2anNPqz/ZJGFtTBW/1onz/yXbO51TsUDPA36MVUuR/zrrEAAAAASUVORK5CYII=",
    "check_off_22": "iVBORw0KGgoAAAANSUhEUgAAACAAAAAWCAYAAAChWZ5EAAACLklEQVR42u1WPWhTURQ+59778mPSNLGI4iAo6NSxlgoOUaqDfxSHNzsIOroIbo+s7iJCdXMw4A+oqZM0UF1EE1QKrXQoDoJD8vqavrzk5d3PwaQ4GHzXCC45cMbv3O+e79xzP6J+OICgEaNcLktTjCIichxHlJh1pVLJcWrqqu8H81pHCgRi4qFgaJBlWbASyTqgFy+ePfEFgGBmHZcADwCPK8vHk1biYSaTPZrZkyKlfl6G/1BAA7Tjd2jL87ajsHNj4VzxQblclrZtR7EIOI4jisWFnNve/nBg/77Dx44c7O7N54xa6bcDvbq+aX373iChxMyF03PvjXR4tlS9+frtRzRdrwsAWmujBIAoisI371bx5OXyCwAc92xBRNTtRfMTmRTykxNCA8TMRqk1SAghpwpZdMPe7MrKp7wRAQaxJSXH0fy3OvZBSklmIpZSmHWAiAg0euAvigj6zzEmMCYwJiD67xf/qiCzWS3RBykAGH0RgTRIGm9CpWS95QccBB0wM2mAYJCDn3nL87WUYqMufS82AQCcTmfvNpqu/3ltU4Vh2BPMYLOM1je+ountiFQ6efvazEwYW7KBeXi6VL1iqcT9QmFSFHIZsiwZ62uC1uR6Prktn1qee+/y+VPXjQzJwMvZth09f1WdZZm41Q6COWit+roOZQEiSCmRTCbXiPWdS2dOPjK2ZL8M0C6wVqvl221LEHlElBsKjrIaBZHn6elDjV1vWSppk8H9Ae+mXLmIuWYmAAAAAElFTkSuQmCC",
    "check_on_22": "iVBORw0KGgoAAAANSUhEUgAAACAAAAAWCAYAAAChWZ5EAAADlUlEQVR42qWWz2tcVRTHP+feN9OkmWQyM/kBCrbFulFKXNQYiuKmC0FwI+PWNklpoRYDQSiIhBBw4bZKWm0jCF3YiCiKf4HGIOYHaFNEg+CiNr8mdjKdSTLv3uPiJSaxdTqvOXDfhcc97/s9553zvQe2TMGwT9N83sb1kW1wAT/ZfbTlsGvo31Q96VWDhwICCYwGltlS6K4enb312/a36iaw7TD/7DPPNVm5njL2qQ31u/k9hISSFKHida3kw4EnZ26N3cjn7evj464uAkNgTnV1tQQmnE4F9sjfodtEtb5UiiCAqvqkMQkrsKFy/ND0z1P1ZsAMgxfCM5nAHlmthlWBpIjYupYxFuessTax4X14QAyoG9Z6UrdNIHrqyXVFJU4hiqCVCqa5BV8pI2DL3qtHuv88dqw1HgEQicGaIMCvFsgMDPLYV9+SeWsQ3dwAYwRFxDmJS+C+6GqBu6UlWk73kz5zDpvLRXsmi4bVOGH8DwFjwLkHE7EBbnmJdG8/uXeHQRVcyN2PLuNWV5EgEfVmDAvu+6/le5hMFi2VIgLWRkBBgC8USJ/qi8ABv7bG4ptnqUx8j6TT4H1s8TK7I9dymYMvv8LjX3xDxweXI2AFSSTxKys0nnhhB7xYZOFsL5WJ77DZbHT2EczsERznSJ/ux3Z20vjiS7S80YtbXsKtLNPwfA8dl0Yj8VmvsHCuj/WffsRm29AwfGT5NjuiClhLcexj/L0SAJmBQTJvX6Sh5wSdV65h0mkQofDeCOuTE9hsLiq8fdhODXiPSaUoff0l4cIdOi6NYnNtZC4MwIUdh5WRIYrXP8W0taPV6n7vr71doM5hMjnWZ6a5/dqrbN78Zc/hlZEh7o5dxeTaYB9pfxCBnQpyIaa5mXBxISqyyR8I/7rNytA7FD+5hm1vrw0u8Rox2LrRAt1NIgwxB5twxSKL5/qQxkbcagGTzdYEV0AVa7yPp4SiZjYpyB4S3iOJRNSelQom1Vy7z1V9g4gX0fk7TU3FugkoiBEdLYWufMCYwKuGuqUAqhrtxqh6r/++/+9SdQKaNGJEef/41FQ1TgbMEzNzv5dFzweCbw2CICEiyTpXQkRS1tpsIhEsVatXDs/OfX4DbLyRLJ+3Mj7u5rue7k4F9uKm9z0ODUBQrXG9CGqjyH/d8PrhoZmbn8UeyXYPpduOf3R1tdpq1RSBllrjmLWabnLSOjlXABjaGnDidME/eXOdkpBXQzQAAAAASUVORK5CYII=",
    "radio_off_22": "iVBORw0KGgoAAAANSUhEUgAAACAAAAAWCAYAAAChWZ5EAAADtElEQVR42q2Wz28bVRDHZ95b/1r/Sg0KQYKgIihqIw6VSjkkrRRaoZZfKgdy7akX1AOIP8C1xIEzJ3LpCQnJlqKkYBM3RBtXtkvaUNQQQxtBW6KmaX7Uzvq3d/e94RBvZRU3zRq+t1m9ffOZmfdmHocuIiIcGhriiUSCYBcREQIAy2Qyu67bTUqXTRkiSgAQmvbzSw1LnjCEPGZZYpBIgqIo2y5FyQYC7hlE/AMAKB6P87GxMdELAHYa9kbxmZmwygJfmqZ1VvWpwYBfBZ/XDQAAhmFCrd4EvVyWLoVfFoI+O3N65DYRcUQUPQPYzi+l5t4Cxf2tX1UP9D8Xghf7I6IvHCTO2eO1lWqNHm4UlbVNHbb1cplIfn7m1LGLvUIAETEAgMnL2SNT6Vwxt1CgRyXdJCJJbUkpScrHJhER1RtNa+HmLUrOXqOJpHbODsSpc4xGoyybzQYnknN3ctcLZBiG+TSnO9+p87v49bdlMTmdlamZ3GGnECyRSLBYLCY3dPFVONy3f+j1QcvlcilEBIgIiPjvuiEAIgIRARGxNw++SpF9YWw0zYv5fN5XKHxCjs6Aps0PbFXq9w69sd916MArSETYzfFTMgiICGvrW2Lx1go3jcb7H747ktpzBgAAyoZ5MhQOeQb699GTN+OZEbQz0f98hAKql1qG9bGT/xkAgGmaJ4N+H/SFQmRv6lScMxb0+1AKOZzP532OAEjSyz6vBxgDJOq5qaGqekCSfIHzPtUZABH1EHTXciAiOS4BY0xvGeZ/Bmg2DUCCmmmi4QhAUXimWmtCtVYn7D0VslJrECrs+sjIwYojAI+bpXW9LNc3S3ZJnDQyQEQolspQrTfR7VK+d1SCeDzOT70zvCylmF1dL7FWyxD21dobwE70f62sYVnXN0NuX7I9pvcGYHetQMDz6aNSqV5YvocAIJ8F0e6CwBjSnb8fiOJ2lSmcnx8dPbJ14cKcs3lg9+6pdOZsavYa3Vi8LQ3DtJ4cRF1mg/zz7n3zp9xNmkjOfdPTMLKlaZoCADA5feXcpfRV88r8It1/sGG1QUR7MkoiEkIIa6u4bc7f+J1+1BZoajrb6dxZJ+00opqmxEZHrR/SV9+WKL92u71H/aoXgn4v+FXPzlVrmVCpNaFWb0K9XlvjCn7xwYnh79ovKQIA6hmg82EyPj7uGnzt8OmWYXwkSByVQgwAATDOKgpXfnF7XCnireR7x49vRqPEYjGU8H/JfqB02HxpaSWSX1qKLKyuqt3OT6/6B1Q8ci2ZFm3pAAAAAElFTkSuQmCC",
    "radio_on_22": "iVBORw0KGgoAAAANSUhEUgAAACAAAAAWCAYAAAChWZ5EAAAEEklEQVR42q1WTWhcVRT+zrlvZjIzbSYJNqZRi020JaNik6kxIBKscSGFgouhm0JXiogLi1LoqhuhUgTBhRCE4iIgzSzFQmupBtzpJJHaxkKSloikpDaZyWQyZubd+7mYn8SxYl7Tu3qP++77vvud851zgAcsAsI0DP5nERACih0secBPVQAHAL8PvvCksXgd5KuW3EcACuRE5Udx/K576sYMADCdNpLJ2B0TqP/o51RPopvxjwU82aK6WwSwZO2AwAhQsM4JcGW1Ih/0Xb9+62FJSDP4b4eee6nNyFiL6oFV38KSVgASIg2NAELEazUK33F1jfZU79TMhYchIVtln+1//nBceMUTbS9Y64uIkTrJBj4bhx1pQ6ompoLlin3n2V9ufhmUhLIGMHPw4O4wOO5pFVxFPAEEqoAq6Pug76P+DgAqYnzSFa1zuzwdvX24r18yGTueTpttE0A6rQK4SMx80uqZ/QW/Cl7dVbBUAotFmEQbTKINLBbBUqlBQgC1JA1EaPXCwtBQNJ3JMFAIbieTXRqROyoS8kmp35zr6wj1PoP2Ux+hZSAFAPhrMouVzz5FZW4WEosBztXCAdvhGbPiu6P7p3+9tH0FAEhER1qNifjOsQFeKiHU04u9YxcROzICbWuHtrUjdmQEe8cuItTT26QESQFV3FtBkrBWRDhSy7FN6ayPjtNnoIkEWClXk48EK2VoIoGO02cA62/RUnTDUUi8sjA0FA1EgHBP2YbNBSyXYTq7EOkfqBrQC1VdIFJ9JhHpH4Dp7ALL5bpDxJIg5HHN5WLBFKAQj24xcAgEyG9WJELCYdilu9iYmgREQL+yGQK/AohgY2oSdukuJBxu1IZaPIsRshwwB3TCNOeA8bB8/hxcPg8JhTdDEArD5fNYPn8OMN6We9NFVKngT3tu3SoEIuCsXC741qlIlZBzkGgUlfk5LJ44jvVrV+FyK3C5Faxfu4rFE8dRmZ+DRKMNGxKAJxAA3wQqxeOASQPuTn/ycqsxb+R9a1XEbC1EcA5mT2fVHPeWANVmcBcRQYW872lLsjubvS/bzAVNnz1LADDAexvWrYdVhaTbqoTE47D5HGw+B4nHm8EppI0bVSvy/hPZ7J8/DA+bYM2o1kDmDyVPJjzz1Zp1dKSTuhL/1Yxq4I+FQt5SxR/tnb7xbtBm1Ej+74eHvdcmJvz5F5Nvxz39QiDemrW26nCR+rcCkCQBMKTq7TKKnLWjT09WwZHJOAlgxX8MJA0Sqb6XYzSftxgdJIENEj4JqWVtRBVGgKJzi751H+6bvvl1bTSjBKwD/x7J0jCSgR1NpUJHpfwmHI+BHHRAV20kK6giq9RLBVf69sD07L2tY9yjKmXaPKrlhpIdC0PJjj9SqVjz3k6w/gbGlgjhABcbkwAAAABJRU5ErkJggg==",
}


# Logo Kometa (PNG, base64)
_LOGO = {"40": "iVBORw0KGgoAAAANSUhEUgAAACgAAAAoCAYAAACM/rhtAAAFUElEQVR42s2ZzW9cVxmHn/d8zNieD9slieU4rdqmbVpC5LQNVcUC6KZNsykSGgR/AGWBhATiX2FBKyEhsUqEgAUBlmWBIDhV0xGV6ha1TT/Sqo3tmfHYnrnnnJfFueNiVNm1MyY+0kizuHPnmd/9vZ8jlEdbWLlC/CVP+u88OXyhSPpDFT0LnAwKAsIYj4K6fMePROVf3sjLv79e+cOPuF6MWKD8Um21rFy5Et9c/Or5aWt/YQ3fSAqFKoXqeMl2QuJF8CIYgZj4WyfGH5+58cZrIya53GrZ72W4p5rO/KUqZqYTQkREABm3cl+kJKCo6rRzdqBprRvSc2duvHHtcqtlBeDf586dq1XSK4rMbsUURMRxF46qhglrnKCr/aH51ul2uy03n356Mm11/9p07sJaCMHcJbjRSaphxjnXDWHJTDS/adKgd6lm7YVOCPFuwwEYEdcJIdasvZAGvUsG9MUEmi13NI6IkLI9XzQKi4WqKJijAqhgSqZF48TMlankyEgoIIUqTsycCap6dB7uDkiCqpqjpNwXKWkO6OJR4jr8qN43mBHEOaRSAWshpf+6m8nXjBF83wqKtfmNtZhaHdNolBk2gQhSqSDejw3S7LMWZVc4t62oTE7h5ucz6GAAKSL1OlKr5etVP7fE/wNQQ0CczyoZU2Ytg50/iXvgQcR5dDDANBrYY8fyNSEcGNLt24MhZJWmaqAJsS7HW4zYmVnciTlSv0fq9pDqBKbRJHx8i7TeQ3xl349+/1FsDGljA3EWqU6Ad1lNa9FQgCru1H34B0+Dd5h6ncpjZ3HzC+hwuDMLHAogQIykbheZnMyqOJeBnQMRdHMTMz2Nm5sn9fvo1ib+/geonHkUUkKL4ktDuoMoKNYSOx3MPV/BzsyiRYF4hzgP3mWIjT7VC1+nduw4qBJ7PcL7N7EzswzfWiatreZg2+ORuwN0leA9xhjiZ5/i5uaQis+KlF/oFhaof7dF5dHHdvZ6vS6D69fZ/OffWf/db9GNjTLQxgmYEsScSnQwIK2v407dh25uQErYkyeZ/enPkampMs2k7cJlGk0mv/0MdmGBYft1tl5dQiYmdyb7O/agSPbQcIhpNkndLrq1iZ2ZQapVmt//QYaLsaw8tnyZDBwClYcfwZ26Fx0M9/TigWtx6vdzynGO8NGHxNUVKovn8Q89klUbVZz/TVNlKaxdvIQ4u6cH76hJjSsriDHI1BTh1i3cseOjwWLXIEOE6vnHscdP5NSzi4oHBzQGYiR8fAvxPudEkXLa/RLHWrDuEBVUBWtJ6+uEDz7ANOqkbqfs4mT3z6mSVlbQXjfnzl0gzR0Osoj32YO3b1O89x7a7+/acmkZPP0//ZG4upKr0GF5cBuyUqF4+y2Kt5fZWrqWAVPMkTzqaDSVCT3/oM6vXkYazQx8qIAjzpQYLi+zee0fFMtvZn9Z+3nkikG8p7j5Lp/+7CekTidXnj08KO88fjaNZS4pOx2p1fAPPYw/dS9Tzz5PdfE8YoR4+zb9P1+l+5tfE1dXMfV6VniPvY28/8TXNIxrgyW57dJQoIMB4hz2+AkQQ+qvk9ZWMfUGOJ8tsMfmy4nggqZPfJ6N9Y6VLCNbnEMmc6lLnQ6KIsZiZu/JZW1vOPUiUmj6xAjc8CIqkMa0otqu16RUtmI+jwqjoNl73Ewl0w0D8pIB0cMaIUdRjO5nDYfJCfUlY6qNq/0Yl6ads0k13O1hPamGaedsP8YlU21cPfILTHO51bKn2+12J+hFYG3aOaeqUXPJP/TVQWmApKpx2jkHrHWCXjzdbre3V8BHeol+1P+G+A9IaMI5EGkYlwAAAABJRU5ErkJggg==", "56": "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", "64": "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"}


class Theme:
    """Spoločný vzhľad: farby, písma, štýly ttk, karty."""

    def __init__(self, root):
        self.root = root
        self._fonts()
        self._style()

    def _fonts(self):
        fams = set(tkfont.families())
        fam = next((f for f in ("Segoe UI", "Helvetica Neue", "Ubuntu", "DejaVu Sans") if f in fams),
                   tkfont.nametofont("TkDefaultFont").actual("family"))
        for name in ("TkDefaultFont", "TkTextFont", "TkMenuFont"):
            tkfont.nametofont(name).configure(family=fam, size=10)
        self.f_title = tkfont.Font(family=fam, size=19, weight="bold")
        self.f_sub = tkfont.Font(family=fam, size=10)
        self.f_head = tkfont.Font(family=fam, size=11, weight="bold")
        self.f_bold = tkfont.Font(family=fam, size=10, weight="bold")
        self.f_small = tkfont.Font(family=fam, size=9)
        self.f_btn = tkfont.Font(family=fam, size=11, weight="bold")
        self.f_mono = tkfont.Font(family="Consolas" if "Consolas" in fams else "DejaVu Sans Mono", size=9)


    def _style(self):
        s = ttk.Style(self.root)
        s.theme_use("clam")
        s.configure(".", background=BG, foreground=TEXT, bordercolor=BORDER,
                    lightcolor=BORDER, darkcolor=BORDER, focuscolor=ACCENT)
        s.configure("TFrame", background=BG)
        s.configure("Card.TFrame", background=CARD)
        s.configure("TLabel", background=BG, foreground=TEXT)
        s.configure("Card.TLabel", background=CARD)
        s.configure("Muted.Card.TLabel", background=CARD, foreground=MUTED, font=self.f_small)
        s.configure("Muted.TLabel", background=BG, foreground=MUTED, font=self.f_small)
        s.configure("Head.Card.TLabel", background=CARD, font=self.f_head)
        s.configure("Info.Card.TLabel", background=CARD, foreground=TEXT, font=self.f_bold)
        s.configure("Order.Card.TLabel", background=CARD, foreground=ACCENT_DARK, font=self.f_bold)

        s.configure("TButton", background="#eef0f3", foreground=TEXT, padding=(12, 6), width=-8,
                    bordercolor="#d3d8df", lightcolor="#eef0f3", darkcolor="#eef0f3", relief="flat")
        s.map("TButton", background=[("disabled", "#f4f5f7"), ("pressed", "#dde1e6"), ("active", "#e4e7ec")],
              lightcolor=[("active", "#e4e7ec")], darkcolor=[("active", "#e4e7ec")],
              foreground=[("disabled", "#a5abb5")])
        s.configure("Soft.TButton", background=ACCENT_SOFT, foreground=ACCENT_DARK, font=self.f_bold,
                    bordercolor="#f6c4c1", lightcolor=ACCENT_SOFT, darkcolor=ACCENT_SOFT, padding=(14, 6))
        s.map("Soft.TButton", background=[("disabled", "#f4f5f7"), ("pressed", "#f9cfcc"), ("active", "#fbdad8")],
              lightcolor=[("active", "#fbdad8")], darkcolor=[("active", "#fbdad8")],
              foreground=[("disabled", "#a5abb5")])
        s.configure("Accent.TButton", background=ACCENT, foreground="white", font=self.f_btn,
                    padding=(26, 10), bordercolor=ACCENT, lightcolor=ACCENT, darkcolor=ACCENT)
        s.map("Accent.TButton",
              background=[("disabled", "#f0b3b0"), ("pressed", ACCENT_DARK), ("active", ACCENT_DARK)],
              bordercolor=[("disabled", "#f0b3b0"), ("active", ACCENT_DARK)],
              lightcolor=[("disabled", "#f0b3b0"), ("active", ACCENT_DARK)],
              darkcolor=[("disabled", "#f0b3b0"), ("active", ACCENT_DARK)],
              foreground=[("disabled", "white")])
        s.configure("Link.TButton", background=CARD, foreground=ACCENT, padding=(6, 2), width=0,
                    bordercolor=CARD, lightcolor=CARD, darkcolor=CARD, font=self.f_small)
        s.map("Link.TButton", background=[("active", ACCENT_SOFT)], foreground=[("disabled", "#c9cdd3")],
              bordercolor=[("active", ACCENT_SOFT)], lightcolor=[("active", ACCENT_SOFT)],
              darkcolor=[("active", ACCENT_SOFT)])
        s.configure("Toggle.TButton", background=BG, foreground=MUTED, padding=(0, 2), width=0,
                    bordercolor=BG, lightcolor=BG, darkcolor=BG, font=self.f_small)
        s.map("Toggle.TButton", background=[("active", BG)], foreground=[("active", TEXT)])

        size = 22 if float(self.root.tk.call("tk", "scaling")) > 1.6 else 16
        self._img = {k: tk.PhotoImage(data=_ICONS[f"{k}_{size}"])
                     for k in ("check_on", "check_off", "radio_on", "radio_off")}
        s.element_create("Pekny.Checkbutton.indicator", "image", self._img["check_off"],
                         ("selected", self._img["check_on"]), sticky="w")
        s.element_create("Pekny.Radiobutton.indicator", "image", self._img["radio_off"],
                         ("selected", self._img["radio_on"]), sticky="w")
        for w, ind in (("TCheckbutton", "Pekny.Checkbutton.indicator"),
                       ("TRadiobutton", "Pekny.Radiobutton.indicator")):
            kind = w[1:]
            s.layout(w, [(f"{kind}.padding", {"sticky": "nswe", "children": [
                (ind, {"side": "left", "sticky": ""}),
                (f"{kind}.label", {"side": "left", "sticky": "nswe"})]})])
            s.configure(w, background=CARD, foreground=TEXT, padding=(2, 3))
            s.map(w, background=[("active", CARD)], foreground=[("disabled", "#a5abb5")])
        s.configure("Orig.TCheckbutton", font=self.f_bold)
        s.layout("Orig.TCheckbutton", s.layout("TCheckbutton"))

        s.configure("TEntry", fieldbackground="white", padding=(8, 6), bordercolor="#cfd4db",
                    lightcolor="#cfd4db", darkcolor="#cfd4db", insertcolor=TEXT)
        s.map("TEntry", bordercolor=[("focus", ACCENT)], lightcolor=[("focus", ACCENT)],
              darkcolor=[("focus", ACCENT)], fieldbackground=[("readonly", "#f8f9fa")])
        s.configure("TCombobox", fieldbackground="white", padding=(6, 4), bordercolor="#cfd4db",
                    arrowcolor=MUTED, lightcolor="#cfd4db", darkcolor="#cfd4db")
        s.map("TCombobox", fieldbackground=[("readonly", "white")], selectbackground=[("readonly", "white")],
              selectforeground=[("readonly", TEXT)], bordercolor=[("focus", ACCENT)])
        s.configure("Horizontal.TProgressbar", background=ACCENT, troughcolor="#e6e8ec",
                    bordercolor="#e6e8ec", lightcolor=ACCENT, darkcolor=ACCENT, thickness=10)
        s.configure("Vertical.TScrollbar", background="#e6e8ec", troughcolor=CARD,
                    bordercolor=CARD, arrowcolor=MUTED, lightcolor="#e6e8ec", darkcolor="#e6e8ec")


    def card(self, parent, number: str, title: str, right=None):
        outer = tk.Frame(parent, bg=CARD, highlightthickness=1, highlightbackground=BORDER,
                         highlightcolor=BORDER, bd=0)
        head = ttk.Frame(outer, style="Card.TFrame")
        head.pack(fill="x", padx=16, pady=(12, 6))
        badge = tk.Label(head, text=number, bg=ACCENT, fg="white", font=self.f_bold, width=2)
        badge.pack(side="left")
        ttk.Label(head, text="  " + title, style="Head.Card.TLabel").pack(side="left")
        if right:
            right(head)
        body = ttk.Frame(outer, style="Card.TFrame")
        body.pack(fill="both", expand=True, padx=16, pady=(0, 14))
        return outer, body



def _extra_styles(theme) -> None:
    """Štýly pre karty (Notebook), tabuľku frontu a ďalšie prvky."""
    s = ttk.Style(theme.root)
    theme.f_small_bold = tkfont.Font(family=theme.f_bold.actual("family"), size=9, weight="bold")
    s.configure("TNotebook", background=BG, borderwidth=0, tabmargins=(22, 6, 22, 0))
    s.configure("TNotebook.Tab", background="#e4e7eb", foreground=MUTED, padding=(18, 8), font=theme.f_bold,
                bordercolor=BORDER, lightcolor="#e4e7eb", darkcolor="#e4e7eb", borderwidth=1)
    s.map("TNotebook.Tab", background=[("selected", BG), ("active", "#eceef1")],
          foreground=[("selected", ACCENT_DARK), ("active", TEXT)],
          lightcolor=[("selected", BG)], darkcolor=[("selected", BG)],
          padding=[("selected", (18, 8))], expand=[("selected", (0, 0, 0, 0))])
    rowh = int(theme.f_sub.metrics("linespace") * 2.1)
    s.configure("Treeview", background=CARD, fieldbackground=CARD, foreground=TEXT, rowheight=rowh,
                bordercolor=CARD, lightcolor=CARD, darkcolor=CARD, borderwidth=0)
    s.layout("Treeview", [("Treeview.treearea", {"sticky": "nswe"})])
    s.map("Treeview", background=[("selected", ACCENT_SOFT)], foreground=[("selected", TEXT)])
    s.configure("Treeview.Heading", background="#f3f4f6", foreground=MUTED, font=theme.f_small_bold,
                relief="flat", padding=(8, 6), bordercolor=BORDER, lightcolor="#f3f4f6", darkcolor="#f3f4f6")
    s.map("Treeview.Heading", background=[("active", "#e9ecf0")])
    s.configure("TSpinbox", fieldbackground="white", padding=(6, 4), bordercolor="#cfd4db", arrowcolor=MUTED,
                lightcolor="#cfd4db", darkcolor="#cfd4db")
    s.configure("Ok.Card.TLabel", background=CARD, foreground="#15803d", font=theme.f_bold)
    s.configure("Err.Card.TLabel", background=CARD, foreground=ACCENT_DARK, font=theme.f_bold)
    s.configure("Warn.TLabel", background=BG, foreground=ACCENT_DARK, font=theme.f_small_bold)
    s.configure("Plain.TCheckbutton", background=BG)
    s.layout("Plain.TCheckbutton", s.layout("TCheckbutton"))
    s.map("Plain.TCheckbutton", background=[("active", BG)])


def _bar(frac: float | None, width: int = 8) -> str:
    if frac is None:
        return "▱" * width
    full = max(0, min(width, round(frac * width)))
    return "▰" * full + "▱" * (width - full) + f"  {frac * 100:3.0f} %"


class LanguagesTab:
    """Karta „Jazykové stopy“ – preložené zvukové stopy YouTube videa."""
    def __init__(self, win, parent):
        self.win = win
        self.root = win.root
        self.t = win.theme
        self.cfg = win.cfg
        self.q: queue.Queue = queue.Queue()
        self.info = None
        self.loaded_url = ""
        self.tracks: list[dict] = []
        self.track_vars: list[tk.BooleanVar] = []
        self.cancel = threading.Event()
        self.busy = False
        self.last_output: Path | None = None
        self._build(parent)
        self.root.after(100, self._poll)

    # ---------- vzhľad ----------
    # ---------- rozloženie ----------
    def _build(self, parent):
        wrap = ttk.Frame(parent, padding=(18, 14, 18, 12))
        wrap.pack(fill="both", expand=True)

        # 1 – Video
        c1, b1 = self.t.card(wrap, "1", "Video")
        c1.pack(fill="x", pady=(0, 10))
        row = ttk.Frame(b1, style="Card.TFrame")
        row.pack(fill="x")
        self.url_var = tk.StringVar()
        self.url_entry = ttk.Entry(row, textvariable=self.url_var)
        self.url_entry.pack(side="left", fill="x", expand=True)
        self.url_entry.bind("<Return>", lambda e: self.load())
        self.paste_btn = ttk.Button(row, text="Vložiť", command=self.paste)
        self.paste_btn.pack(side="left", padx=(8, 0))
        self.load_btn = ttk.Button(row, text="Načítať jazyky", style="Soft.TButton", command=self.load)
        self.load_btn.pack(side="left", padx=(8, 0))
        self.info_lbl = ttk.Label(b1, text="Vlož odkaz na YouTube video a klikni na „Načítať jazyky“.",
                                  style="Muted.Card.TLabel")
        self.info_lbl.pack(anchor="w", pady=(8, 0))
        self.url_var.trace_add("write", lambda *_: self._url_changed())

        # 2 – Jazyky
        def lang_tools(parent):
            box = ttk.Frame(parent, style="Card.TFrame")
            box.pack(side="right")
            self.sel_btns = []
            for txt, cmd in (("Predvolené", self.select_default), ("Všetky", lambda: self.select_all(True)),
                             ("Žiadne", lambda: self.select_all(False))):
                b = ttk.Button(box, text=txt, style="Link.TButton", command=cmd, state="disabled")
                b.pack(side="left", padx=(4, 0))
                self.sel_btns.append(b)

        c2, b2 = self.t.card(wrap, "2", "Jazyky", right=lang_tools)
        self.lang_canvas = tk.Canvas(b2, bg=CARD, highlightthickness=0, height=96)
        self.lang_scroll = ttk.Scrollbar(b2, orient="vertical", command=self.lang_canvas.yview)
        self.lang_inner = ttk.Frame(self.lang_canvas, style="Card.TFrame")
        self.lang_inner.bind("<Configure>", lambda e: self._update_scroll())
        self._inner_id = self.lang_canvas.create_window((0, 0), window=self.lang_inner, anchor="nw")
        self.lang_canvas.configure(yscrollcommand=self.lang_scroll.set)
        self.lang_canvas.bind("<Configure>", lambda e: (self.lang_canvas.itemconfigure(self._inner_id, width=e.width),
                                                        self._update_scroll()))
        self.order_var = tk.StringVar(value="")
        ttk.Label(b2, textvariable=self.order_var, style="Order.Card.TLabel").pack(side="bottom", anchor="w",
                                                                                 pady=(6, 0))
        self.lang_canvas.pack(side="left", fill="both", expand=True)
        self.lang_canvas.bind("<Enter>", lambda e: self._bind_wheel(True))
        self.lang_canvas.bind("<Leave>", lambda e: self._bind_wheel(False))
        self.lang_hint = ttk.Label(self.lang_inner, text="Tu sa po načítaní zobrazia dostupné jazyky.",
                                   style="Muted.Card.TLabel")
        self.lang_hint.grid(row=0, column=0, sticky="w", pady=4)

        # 3 – Uloženie
        c3, b3 = self.t.card(wrap, "3", "Uloženie")
        self.mode_var = tk.StringVar(value=self.cfg.get("mode", "audio"))

        ttk.Radiobutton(b3, text="Video + jazyky – jedno video .mkv so všetkými stopami", variable=self.mode_var,
                        value="video", command=self._mode_changed).grid(row=0, column=0, sticky="w")
        qbox = ttk.Frame(b3, style="Card.TFrame")
        qbox.grid(row=0, column=1, columnspan=3, sticky="w", padx=(12, 0))
        ttk.Label(qbox, text="kvalita:", style="Muted.Card.TLabel").pack(side="left", padx=(0, 6))
        self.quality_var = tk.StringVar(value=self.cfg.get("max_height", "Najlepšia"))
        self.quality_cb = ttk.Combobox(qbox, textvariable=self.quality_var, values=QUALITIES,
                                       state="readonly", width=10)
        self.quality_cb.pack(side="left")

        ttk.Radiobutton(b3, text="Pridať jazyky k môjmu stiahnutému videu (.mkv)", variable=self.mode_var,
                        value="existing", command=self._mode_changed).grid(row=1, column=0, sticky="w")
        ebox = ttk.Frame(b3, style="Card.TFrame")
        ebox.grid(row=1, column=1, columnspan=3, sticky="ew", padx=(12, 0))
        self.existing_var = tk.StringVar()
        self.existing_entry = ttk.Entry(ebox, textvariable=self.existing_var)
        self.existing_entry.pack(side="left", fill="x", expand=True)
        self.existing_btn = ttk.Button(ebox, text="Vybrať…", command=self.pick_existing)
        self.existing_btn.pack(side="left", padx=(6, 0))

        ttk.Radiobutton(b3, text="Len zvuk – súbor .mka so všetkými jazykmi (bez obrazu)",
                        variable=self.mode_var, value="audio", command=self._mode_changed).grid(
            row=2, column=0, columnspan=4, sticky="w")

        sep = tk.Frame(b3, bg=BORDER, height=1)
        sep.grid(row=3, column=0, columnspan=4, sticky="ew", pady=10)

        opts = ttk.Frame(b3, style="Card.TFrame")
        opts.grid(row=4, column=0, columnspan=4, sticky="w")
        self.orig_var = tk.BooleanVar(value=self.cfg.get("include_original", True))
        self.orig_cb = ttk.Checkbutton(opts, text="Pridať aj pôvodnú stopu (ako poslednú)", variable=self.orig_var,
                                       command=self._set_go_state)
        self.orig_cb.pack(side="left")
        self.sep_var = tk.BooleanVar(value=self.cfg.get("keep_separate", False))
        ttk.Checkbutton(opts, text="Uložiť aj každý jazyk ako samostatný súbor",
                        variable=self.sep_var).pack(side="left", padx=(22, 0))
        ttk.Checkbutton(b3, text="Kompatibilný zvuk – ku každému jazyku pridať AAC (predvolené, prehrá ho každý "
                                 "prehrávač), Opus ostane navyše", variable=self.win.compat_var,
                        command=self._set_go_state).grid(row=5, column=0, columnspan=4, sticky="w", pady=(4, 0))

        ttk.Label(b3, text="Priečinok:", style="Card.TLabel").grid(row=6, column=0, sticky="w", pady=(10, 0))
        dbox = ttk.Frame(b3, style="Card.TFrame")
        dbox.grid(row=7, column=0, columnspan=4, sticky="ew", pady=(4, 0))
        self.outdir_var = self.win.outdir_var
        self.outdir_entry = ttk.Entry(dbox, textvariable=self.outdir_var)
        self.outdir_entry.pack(side="left", fill="x", expand=True)
        ttk.Button(dbox, text="Zmeniť…", command=self.win.pick_outdir).pack(side="left", padx=(6, 0))
        ttk.Button(dbox, text="Otvoriť", command=self.win.open_outdir).pack(side="left", padx=(6, 0))
        b3.columnconfigure(3, weight=1)

        # Spodok – priebeh a tlačidlá
        bottom = ttk.Frame(wrap)
        foot = ttk.Frame(bottom)
        foot.pack(fill="x")
        left = ttk.Frame(foot)
        left.pack(side="left", fill="x", expand=True, padx=(0, 16))
        self.pbar = ttk.Progressbar(left, mode="determinate", maximum=1000)
        self.pbar.pack(fill="x", pady=(6, 4))
        self.status_var = tk.StringVar(value="Pripravené.")
        ttk.Label(left, textvariable=self.status_var, style="Muted.TLabel").pack(anchor="w")
        self.go_btn = ttk.Button(foot, text="Stiahnuť", style="Accent.TButton", command=self.start)
        self.go_btn.pack(side="right")
        self.cancel_btn = ttk.Button(foot, text="Zrušiť", command=self.do_cancel)

        self.log_toggle = ttk.Button(bottom, text="▸ Podrobnosti", style="Toggle.TButton", command=self._toggle_log)
        self.log_toggle.pack(anchor="w", pady=(8, 0))
        self.log_frame = tk.Frame(bottom, bg=CARD, highlightthickness=1, highlightbackground=BORDER)
        self.log_text = tk.Text(self.log_frame, height=7, bg=CARD, fg=TEXT, relief="flat", bd=0,
                                font=self.t.f_mono, wrap="word", padx=10, pady=8, state="disabled")
        self.log_text.pack(fill="both", expand=True)

        # poradie balenia: spodná lišta a karta 3 držia miesto, karta 2 sa prispôsobí
        bottom.pack(side="bottom", fill="x")
        c3.pack(side="bottom", fill="x", pady=(0, 12))
        c2.pack(fill="both", expand=True, pady=(0, 10))

        self._mode_changed()
        self._set_go_state()

    # ---------- jazyky ----------
    def _update_scroll(self):
        self.lang_canvas.configure(scrollregion=self.lang_canvas.bbox("all"))
        need = self.lang_inner.winfo_reqheight() > self.lang_canvas.winfo_height() + 2
        if need and not self.lang_scroll.winfo_ismapped():
            self.lang_scroll.pack(side="right", fill="y")
        elif not need and self.lang_scroll.winfo_ismapped():
            self.lang_scroll.pack_forget()
            self.lang_canvas.yview_moveto(0)

    def _bind_wheel(self, on: bool):
        if on:
            self.root.bind_all("<MouseWheel>", self._wheel)
            self.root.bind_all("<Button-4>", lambda e: self.lang_canvas.yview_scroll(-1, "units"))
            self.root.bind_all("<Button-5>", lambda e: self.lang_canvas.yview_scroll(1, "units"))
        else:
            for ev in ("<MouseWheel>", "<Button-4>", "<Button-5>"):
                self.root.unbind_all(ev)

    def _wheel(self, e):
        if self.lang_scroll.winfo_ismapped():
            step = -1 if e.delta > 0 else 1
            self.lang_canvas.yview_scroll(step * (1 if abs(e.delta) < 120 else abs(e.delta) // 120), "units")

    def _show_tracks(self, tracks: list[dict]):
        for w in self.lang_inner.winfo_children():
            w.destroy()
        self.tracks, self.track_vars = tracks, []
        cols = 3
        for i, t in enumerate(tracks):
            var = tk.BooleanVar(value=False)
            text = f"{t['label']}  ·  {t['code'] or '—'}"
            if t["original"]:
                text = f"★ {t['label']} (originál)  ·  {t['code'] or '—'}"
            cb = ttk.Checkbutton(self.lang_inner, text=text, variable=var,
                                 style="Orig.TCheckbutton" if t["original"] else "TCheckbutton",
                                 command=self._set_go_state)
            cb.grid(row=i // cols, column=i % cols, sticky="w", padx=(0, 18), pady=1)
            self.track_vars.append(var)
        for c in range(cols):
            self.lang_inner.columnconfigure(c, weight=1, uniform="lang")
        if len(tracks) <= 1:
            ttk.Label(self.lang_inner, style="Muted.Card.TLabel",
                      text="Toto video nemá preložené zvukové stopy – je dostupná len pôvodná.").grid(
                row=1, column=0, columnspan=3, sticky="w", pady=(6, 0))
        for b in self.sel_btns:
            b.configure(state="normal")
        self.select_default()

    def _clear_tracks(self, hint: str):
        for w in self.lang_inner.winfo_children():
            w.destroy()
        self.tracks, self.track_vars = [], []
        ttk.Label(self.lang_inner, text=hint, style="Muted.Card.TLabel").grid(row=0, column=0, sticky="w", pady=4)
        for b in self.sel_btns:
            b.configure(state="disabled")
        self._set_go_state()

    def select_default(self):
        chosen, _ = pick_tracks(self.tracks, self.cfg.get("langs") or DEFAULT_LANGS)
        codes = {t["code"] for t in chosen}
        for t, v in zip(self.tracks, self.track_vars):
            v.set(t["code"] in codes)
        self._set_go_state()

    def select_all(self, value: bool):
        for v in self.track_vars:
            v.set(value)
        self._set_go_state()

    def selected_tracks(self) -> list[dict]:
        """Zaškrtnuté stopy – v poradí obľúbených jazykov (cs, sk, en, pl, hu, ru), originál na konci."""
        pref = [c.lower() for c in (self.cfg.get("langs") or DEFAULT_LANGS)]
        sel = [(i, t) for i, (t, v) in enumerate(zip(self.tracks, self.track_vars)) if v.get()]

        def key(item):
            i, t = item
            c = t["code"].lower()
            return (t["original"], pref.index(c) if c in pref else len(pref), i)

        return [t for _, t in sorted(sel, key=key)]

    # ---------- akcie ----------
    def paste(self):
        try:
            text = self.root.clipboard_get().strip()
        except Exception:
            text = ""
        if text:
            self.url_var.set(text)
            self.load()

    def _url_changed(self):
        if self.info is not None and self.url_var.get().strip() != self.loaded_url:
            self.info = None
            self.info_lbl.configure(text="Odkaz sa zmenil – klikni na „Načítať jazyky“.", style="Muted.Card.TLabel")
            self._clear_tracks("Tu sa po načítaní zobrazia dostupné jazyky.")

    def load(self):
        url = self.url_var.get().strip()
        if self.busy:
            return
        if not re.match(r"^(https?://)?([\w-]+\.)*(youtube\.com|youtu\.be)/", url):
            messagebox.showwarning(APP_NAME, "Vlož platný odkaz na YouTube video,\n"
                                              "napr. https://www.youtube.com/watch?v=…")
            return
        self._busy(True, loading=True)
        self.status_var.set("Načítavam informácie o videu… (môže to trvať pár sekúnd)")
        self.info_lbl.configure(text="Načítavam…", style="Muted.Card.TLabel")
        self._log(f"— Načítavam {url}")

        def work():
            try:
                info = fetch_info(url, log=lambda m: self.q.put(("log", m)))
                self.q.put(("info", info, url))
            except Exception as e:
                self.q.put(("load_error", ANSI.sub("", str(e)).replace("ERROR: ", "")))

        threading.Thread(target=work, daemon=True).start()

    def start(self):
        if self.busy:
            return
        if self.info is None:
            messagebox.showinfo(APP_NAME, "Najprv vlož odkaz a načítaj jazyky.")
            return
        tracks = self.selected_tracks()
        mode = self.mode_var.get()
        if not tracks and not self.orig_var.get():
            messagebox.showinfo(APP_NAME, "Zaškrtni aspoň jeden jazyk.")
            return
        if mode == "existing" and not Path(self.existing_var.get().strip()).is_file():
            messagebox.showinfo(APP_NAME, "Vyber svoje stiahnuté video (tlačidlo „Vybrať…“).")
            return
        outdir = self.outdir_var.get().strip() or str(default_outdir())
        try:
            Path(outdir).mkdir(parents=True, exist_ok=True)
        except Exception as e:
            messagebox.showerror(APP_NAME, f"Do priečinka sa nedá ukladať:\n{e}")
            return
        if not shutil.which("ffmpeg"):
            messagebox.showerror(APP_NAME, "Nenašiel som FFmpeg. Spusti Instalacia.bat a potom program znova.")
            return

        q = self.quality_var.get()
        self.cfg.update({"outdir": outdir, "langs": [t["code"] for t in tracks if t["code"]] or self.cfg["langs"],
                         "mode": mode, "include_original": self.orig_var.get(),
                         "keep_separate": self.sep_var.get(), "max_height": q})
        save_config(self.cfg)
        job = {"info": self.info, "tracks": tracks, "outdir": outdir, "mode": mode,
               "existing_path": self.existing_var.get().strip(), "include_original": self.orig_var.get(),
               "keep_separate": self.sep_var.get(),
               "audio_mode": "oboje" if self.win.compat_var.get() else "opus",
               "max_height": int(q[:-1]) if q.endswith("p") and q[:-1].isdigit() else None}
        self.cancel = threading.Event()
        self._busy(True)
        self._log("— Začínam sťahovať")

        def work():
            try:
                out = run_job(job,
                              log=lambda m: self.q.put(("log", m)),
                              progress=lambda f, t: self.q.put(("progress", f, t)),
                              cancel=self.cancel)
                self.q.put(("done", out))
            except Cancelled:
                self.q.put(("cancelled",))
            except Exception as e:
                if self.cancel.is_set():
                    self.q.put(("cancelled",))
                else:
                    self.q.put(("error", ANSI.sub("", str(e)).replace("ERROR: ", "")))

        threading.Thread(target=work, daemon=True).start()

    def do_cancel(self):
        self.cancel.set()
        self.status_var.set("Ruším…")
        self.cancel_btn.configure(state="disabled")

    def pick_existing(self):
        f = filedialog.askopenfilename(
            title="Vyber svoje stiahnuté video",
            initialdir=self.outdir_var.get() or str(Path.home()),
            filetypes=[("Video", "*.mp4 *.mkv *.webm *.mov *.avi *.m4v"), ("Všetky súbory", "*.*")])
        if f:
            self.existing_var.set(str(Path(f)))
            self.mode_var.set("existing")
            self._mode_changed()

    # ---------- stav ----------
    def _mode_changed(self):
        mode = self.mode_var.get()
        self.quality_cb.configure(state="readonly" if mode == "video" else "disabled")
        st = "normal" if mode == "existing" else "disabled"
        self.existing_entry.configure(state=st)
        self.existing_btn.configure(state="normal")
        self._set_go_state()

    def _update_order(self):
        if not self.tracks:
            self.order_var.set("")
            return
        order = self.selected_tracks()
        orig = next((t for t in self.tracks if t["original"]), None)
        if self.orig_var.get() and orig and orig not in order:
            order.append(orig)
        short = lambda t: {"cs": "CZ"}.get(t["code"].lower(), (t["code"] or "orig").upper())
        text = "Poradie stôp vo výsledku:  " + "  →  ".join(short(t) for t in order)
        if order and self.win.compat_var.get():
            text += "   (AAC, potom to isté v Opus)"
        self.order_var.set(text if order else "Nie je vybraný žiadny jazyk.")

    def _set_go_state(self):
        self._update_order()
        if self.busy:
            return
        ok = self.info is not None and (any(v.get() for v in self.track_vars) or self.orig_var.get())
        self.go_btn.configure(state="normal" if ok else "disabled")

    def _busy(self, on: bool, loading: bool = False):
        self.busy = on
        for w in (self.load_btn, self.paste_btn, self.url_entry):
            w.configure(state="disabled" if on else "normal")
        if on:
            self.go_btn.configure(state="disabled")
            if loading:
                self.pbar.configure(mode="indeterminate")
                self.pbar.start(12)
            else:
                self.pbar.configure(mode="determinate", value=0)
                self.cancel_btn.configure(state="normal")
                self.cancel_btn.pack(side="right", padx=(0, 8))
        else:
            self.pbar.stop()
            self.pbar.configure(mode="determinate")
            self.cancel_btn.pack_forget()
            self._set_go_state()

    def _toggle_log(self):
        extra = 140
        w, h = self.root.winfo_width(), self.root.winfo_height()
        if self.log_frame.winfo_ismapped():
            self.log_frame.pack_forget()
            self.log_toggle.configure(text="▸ Podrobnosti")
            if getattr(self, "_grown", False):
                self.root.geometry(f"{w}x{h - extra}")
                self._grown = False
        else:
            self.log_frame.pack(fill="both", expand=False, pady=(4, 0))
            self.log_toggle.configure(text="▾ Podrobnosti")
            if self.root.state() == "normal" and h + extra < self.root.winfo_screenheight() - 60:
                self.root.geometry(f"{w}x{h + extra}")
                self._grown = True

    def _log(self, msg: str):
        self.log_text.configure(state="normal")
        self.log_text.insert("end", msg + "\n")
        self.log_text.see("end")
        self.log_text.configure(state="disabled")

    def _poll(self):
        try:
            while True:
                item = self.q.get_nowait()
                kind = item[0]
                if kind == "log":
                    self._log(item[1])
                elif kind == "progress":
                    self.pbar.configure(value=int(item[1] * 1000))
                    if item[2]:
                        self.status_var.set(f"{item[2]}   {item[1] * 100:.0f} %")
                elif kind == "info":
                    self._on_info(item[1], item[2])
                elif kind == "load_error":
                    self._busy(False)
                    self.status_var.set("Načítanie zlyhalo.")
                    self.info_lbl.configure(text="Video sa nepodarilo načítať.", style="Muted.Card.TLabel")
                    self._log("✖ " + item[1])
                    messagebox.showerror(APP_NAME, "Video sa nepodarilo načítať:\n\n" + item[1][:900] +
                                         "\n\nTip: spusti Spustit.bat znova – aktualizuje yt-dlp.")
                elif kind == "done":
                    self._busy(False)
                    self.pbar.configure(value=1000)
                    self.last_output = Path(item[1])
                    self.status_var.set(f"Hotovo ✓  {self.last_output.name}")
                    self.root.bell()
                    if messagebox.askyesno(APP_NAME, f"Hotovo!\n\n{self.last_output.name}\n\nOtvoriť priečinok?"):
                        open_folder(self.last_output.parent)
                elif kind == "cancelled":
                    self._busy(False)
                    self.pbar.configure(value=0)
                    self.status_var.set("Zrušené.")
                    self._log("— Zrušené")
                elif kind == "error":
                    self._busy(False)
                    self.status_var.set("Chyba – pozri Podrobnosti.")
                    self._log("✖ " + item[1])
                    if not self.log_frame.winfo_ismapped():
                        self._toggle_log()
                    messagebox.showerror(APP_NAME, item[1][:1200])
        except queue.Empty:
            pass
        self.root.after(100, self._poll)

    def _on_info(self, info: dict, url: str):
        self._busy(False)
        self.info, self.loaded_url = info, url
        tracks = list_audio_tracks(info)
        orig = next((t for t in tracks if t["original"]), None)
        dur = format_duration(info.get("duration"))
        parts = [p for p in (dur, f"originál: {orig['label']}" if orig else "",
                             f"{len(tracks)} jazykov" if len(tracks) != 1 else "1 jazyk") if p]
        title = info.get("title") or info.get("id")
        self.info_lbl.configure(text=f"▶  {title}\n     " + "  ·  ".join(parts), style="Info.Card.TLabel",
                                wraplength=max(self.info_lbl.winfo_width(), 600))
        self.orig_cb.configure(text=f"Pridať aj pôvodnú stopu – {orig['label']} (ako poslednú)" if orig else
                               "Pridať aj pôvodnú stopu (ako poslednú)")
        self._show_tracks(tracks)
        self.status_var.set("Vyber jazyky a klikni na „Stiahnuť“.")
        self._log(f"✔ {title} – {len(tracks)} zvukových stôp")


    def prefill(self):
        """Pri otvorení karty vloží odkaz na YouTube zo schránky (ak tam je)."""
        try:
            clip = self.root.clipboard_get().strip()
        except Exception:
            return
        if re.match(r"^https?://([\w-]+\.)*(youtube\.com|youtu\.be)/", clip) and not self.url_var.get():
            self.url_var.set(clip)


class DownloadsTab:
    """Karta „Sťahovanie“ – front odkazov z YouTube, Facebooku, Instagramu, TikToku, Webshare, Fastshare…"""

    COLS = (("title", "Názov", 330, "w"), ("source", "Zdroj", 90, "w"), ("size", "Veľkosť", 130, "e"),
            ("progress", "Priebeh", 175, "w"), ("speed", "Rýchlosť", 90, "e"), ("status", "Stav", 130, "w"))
    HINT = "Sem vlož jeden alebo viac odkazov – každý na nový riadok…"

    def __init__(self, win, parent):
        self.win = win
        self.root = win.root
        self.t = win.theme
        self.cfg = win.cfg
        self.dq = win.dq
        self._build(parent)

    # ---------- rozloženie ----------
    def _build(self, parent):
        wrap = ttk.Frame(parent, padding=(18, 14, 18, 12))
        wrap.pack(fill="both", expand=True)

        c1, b1 = self.t.card(wrap, "1", "Odkazy")
        c1.pack(fill="x", pady=(0, 10))
        row = ttk.Frame(b1, style="Card.TFrame")
        row.pack(fill="x")
        box = tk.Frame(row, bg="white", highlightthickness=1, highlightbackground="#cfd4db", highlightcolor=ACCENT)
        box.pack(side="left", fill="both", expand=True)
        self.links = tk.Text(box, height=6, width=40, relief="flat", bd=0, bg="white", fg=TEXT, wrap="none",
                             font=self.t.f_sub, padx=8, pady=6, insertbackground=TEXT, undo=True)
        self.links.pack(fill="both", expand=True)
        self.links.tag_configure("hint", foreground="#9aa1ab")
        self._show_hint()
        self.links.bind("<FocusIn>", lambda e: self._hide_hint())
        self.links.bind("<FocusOut>", lambda e: self._show_hint())
        self.links.bind("<Control-Return>", lambda e: (self.add(), "break")[1])
        side = ttk.Frame(row, style="Card.TFrame")
        side.pack(side="left", fill="y", padx=(10, 0))
        ttk.Button(side, text="Vložiť zo schránky", command=self.paste).pack(fill="x")
        ttk.Button(side, text="Playlist / kanál…", style="Soft.TButton", command=self.open_playlist).pack(
            fill="x", pady=(6, 0))
        ttk.Button(side, text="Prehľadať stránku…", style="Soft.TButton", command=self.scan_page).pack(
            fill="x", pady=(6, 0))
        ttk.Button(side, text="Stiahnuť", style="Accent.TButton", command=self.add).pack(fill="x", pady=(6, 0))

        opts = ttk.Frame(b1, style="Card.TFrame")
        opts.pack(fill="x", pady=(10, 0))
        ttk.Label(opts, text="Formát:", style="Card.TLabel").pack(side="left")
        self.preset_keys = list(media.PRESETS)
        labels = [media.PRESETS[k][0] for k in self.preset_keys]
        cur = self.cfg.get("dl_preset", "video")
        self.preset_var = tk.StringVar(value=media.PRESETS.get(cur, media.PRESETS["video"])[0])
        cb = ttk.Combobox(opts, textvariable=self.preset_var, values=labels, state="readonly", width=34)
        cb.pack(side="left", padx=(6, 18))
        cb.bind("<<ComboboxSelected>>", lambda e: self._save_preset())
        ttk.Label(opts, text="Priečinok:", style="Card.TLabel").pack(side="left")
        ttk.Entry(opts, textvariable=self.win.outdir_var).pack(side="left", fill="x", expand=True, padx=(6, 6))
        ttk.Button(opts, text="Zmeniť…", command=self.win.pick_outdir).pack(side="left")
        ttk.Label(b1, style="Muted.Card.TLabel",
                  text="YouTube (aj celé playlisty a kanály) · Facebook · Instagram · TikTok (bez vodoznaku) · "
                       "Webshare · Fastshare · "
                       "ďalšie stránky s videom aj priame odkazy na súbory. Prerušené sťahovanie pokračuje "
                       "tam, kde skončilo. Na iných stránkach (kurzy, články, Vimeo vo vložených prehrávačoch) "
                  "nájde video skener – automaticky alebo cez „Prehľadať stránku…“.", wraplength=860,
                  justify="left").pack(anchor="w", pady=(8, 0))

        foot = ttk.Frame(wrap)
        foot.pack(side="bottom", fill="x", pady=(8, 0))
        self.summary_var = tk.StringVar(value="")
        ttk.Label(foot, textvariable=self.summary_var, style="Muted.TLabel").pack(side="left")
        ttk.Label(foot, text="Dvojklik: otvoriť / pozastaviť / pokračovať · pravé tlačidlo: ďalšie možnosti",
                  style="Muted.TLabel").pack(side="right")

        c2, b2 = self.t.card(wrap, "2", "Front sťahovania")
        c2.pack(fill="both", expand=True)
        tv_box = ttk.Frame(b2, style="Card.TFrame")
        tv_box.pack(fill="both", expand=True)
        self.tv = ttk.Treeview(tv_box, columns=[c[0] for c in self.COLS], show="headings", selectmode="extended")
        for key, text, width, anchor in self.COLS:
            self.tv.heading(key, text=text, anchor=anchor)
            self.tv.column(key, width=width, minwidth=60, anchor=anchor, stretch=(key == "title"))
        sb = ttk.Scrollbar(tv_box, orient="vertical", command=self.tv.yview)
        self.tv.configure(yscrollcommand=sb.set)
        self.tv.pack(side="left", fill="both", expand=True)
        sb.pack(side="right", fill="y")
        self.tv.tag_configure("hotovo", foreground="#15803d")
        self.tv.tag_configure("chyba", foreground=ACCENT_DARK)
        self.tv.tag_configure("pauza", foreground=MUTED)
        self.tv.tag_configure("caka", foreground=MUTED)
        self.tv.tag_configure("rozbalene", foreground=MUTED)
        self.tv.bind("<Double-1>", self._on_double)
        self.tv.bind("<Delete>", lambda e: self.remove())
        self.tv.bind("<Button-3>", self._context)
        self.tv.bind("<Button-2>", self._context)
        self.empty_lbl = ttk.Label(self.tv, text="Front je prázdny. Vlož odkazy hore a klikni na „Stiahnuť“.",
                                   style="Muted.Card.TLabel")

        bar = ttk.Frame(b2, style="Card.TFrame")
        bar.pack(fill="x", pady=(10, 0))
        for text, cmd in (("▶  Spustiť", self.start), ("❚❚  Pozastaviť", self.pause),
                          ("✕  Odstrániť", self.remove), ("Vyčistiť hotové", self.dq.clear_finished)):
            ttk.Button(bar, text=text, command=cmd).pack(side="left", padx=(0, 6))
        ttk.Button(bar, text="Otvoriť priečinok", command=self.win.open_outdir).pack(side="right")

        self.menu = tk.Menu(self.root, tearoff=0)
        self.menu.add_command(label="Spustiť / pokračovať", command=self.start)
        self.menu.add_command(label="Pozastaviť", command=self.pause)
        self.menu.add_separator()
        self.menu.add_command(label="Otvoriť priečinok so súborom", command=self._open_selected)
        self.menu.add_command(label="Kopírovať odkaz", command=self._copy_url)
        self.menu.add_command(label="Zobraziť chybu", command=self._show_error)
        self.menu.add_separator()
        self.menu.add_command(label="Odstrániť", command=self.remove)

        for iid in self.dq.ids():
            self.refresh(iid)
        self._update_summary()

    # ---------- pole s odkazmi ----------
    def _show_hint(self):
        if not self.links.get("1.0", "end").strip():
            self.links.delete("1.0", "end")
            self.links.insert("1.0", self.HINT, "hint")

    def _hide_hint(self):
        if self.links.tag_ranges("hint"):
            self.links.delete("1.0", "end")

    def _get_links(self) -> list[str]:
        if self.links.tag_ranges("hint"):
            return []
        text = self.links.get("1.0", "end")
        return [u for u in re.findall(r"https?://\S+", text)]

    def paste(self):
        try:
            text = self.root.clipboard_get()
        except Exception:
            return
        self._hide_hint()
        cur = self.links.get("1.0", "end").strip()
        self.links.insert("end", ("\n" if cur else "") + text.strip())

    def _save_preset(self):
        self.cfg["dl_preset"] = self._preset()
        save_config(self.cfg)

    def _preset(self) -> str:
        label = self.preset_var.get()
        return next((k for k in self.preset_keys if media.PRESETS[k][0] == label), "video")

    # ---------- akcie ----------
    def add(self):
        urls = self._get_links()
        if not urls:
            messagebox.showinfo(APP_NAME, "Vlož aspoň jeden odkaz (začína na http:// alebo https://).")
            return
        outdir = self.win.outdir_var.get().strip() or str(default_outdir())
        try:
            Path(outdir).mkdir(parents=True, exist_ok=True)
        except Exception as e:
            messagebox.showerror(APP_NAME, f"Do priečinka sa nedá ukladať:\n{e}")
            return
        self._save_preset()
        collections = [u for u in urls if media.is_collection(u)]
        urls = [u for u in urls if u not in collections]
        new = self.dq.add(urls, self._preset(), outdir) if urls else []
        self.links.delete("1.0", "end")
        self._show_hint()
        for u in collections:          # playlist alebo kanál → okno s výberom videí
            PlaylistDialog(self.win, u)
        if not urls:
            return
        skipped = len(urls) - len(new)
        self.win.set_status(f"Pridané do frontu: {len(new)}" + (f" (už vo fronte: {skipped})" if skipped else ""))

    def scan_page(self):
        urls = self._get_links()
        if urls and media.is_collection(urls[0]):
            PlaylistDialog(self.win, urls[0])
            return
        ScanDialog(self.win, urls[0] if urls else "")

    def open_playlist(self):
        urls = self._get_links()
        PlaylistDialog(self.win, urls[0] if urls else "")

    def _selected(self) -> list[str]:
        return list(self.tv.selection())

    def start(self):
        ids = self._selected() or self.dq.ids()
        self.dq.start(ids)

    def pause(self):
        ids = self._selected() or self.dq.ids()
        self.dq.pause(ids)

    def remove(self):
        ids = self._selected()
        if not ids:
            return
        unfinished = [i for i in ids if (self.dq.snapshot(i) or {}).get("status") != "hotovo"]
        if unfinished and not messagebox.askyesno(
                APP_NAME, f"Odstrániť {len(ids)} položiek z frontu?\n\n"
                          "Rozpracované súbory sa zmažú. Hotové súbory na disku ostanú."):
            return
        threading.Thread(target=self.dq.remove, args=(ids,), daemon=True).start()

    def _context(self, e):
        row = self.tv.identify_row(e.y)
        if row and row not in self.tv.selection():
            self.tv.selection_set(row)
        try:
            self.menu.tk_popup(e.x_root, e.y_root)
        finally:
            self.menu.grab_release()

    def _on_double(self, e):
        row = self.tv.identify_row(e.y)
        it = self.dq.snapshot(row) if row else None
        if not it:
            return
        if it["status"] == "hotovo":
            self._open_selected()
        elif it["status"] == "chyba":
            self._show_error()
        elif it["status"] in ("pauza",):
            self.dq.start([row])
        elif it["status"] in ("stahuje", "caka"):
            self.dq.pause([row])

    def _open_selected(self):
        ids = self._selected()
        it = self.dq.snapshot(ids[0]) if ids else None
        if it and it.get("file"):
            open_folder(Path(it["file"]).parent, select=Path(it["file"]))
        else:
            self.win.open_outdir()

    def _copy_url(self):
        urls = [self.dq.snapshot(i)["url"] for i in self._selected() if self.dq.snapshot(i)]
        if urls:
            self.root.clipboard_clear()
            self.root.clipboard_append("\n".join(urls))

    def _show_error(self):
        for i in self._selected():
            it = self.dq.snapshot(i)
            if it and it.get("error"):
                messagebox.showerror(APP_NAME, f"{it.get('title') or it['url']}\n\n{it['error']}")
                return

    # ---------- obnova tabuľky ----------
    def refresh(self, iid: str):
        it = self.dq.snapshot(iid)
        if it is None:
            if self.tv.exists(iid):
                self.tv.delete(iid)
            return
        frac = (it["done"] / it["total"]) if it.get("total") else None
        if it["status"] == "hotovo":
            frac = 1.0
        size = human_size(it.get("total"))
        if it["status"] == "stahuje" and it.get("done"):
            size = f"{human_size(it['done'])} / {size}" if size else human_size(it["done"])
        status = STATUS_TEXT.get(it["status"], it["status"])
        if it["status"] == "stahuje" and it.get("speed") and it.get("total"):
            eta = human_eta((it["total"] - it["done"]) / it["speed"])
            status += f" · {eta}" if eta else ""
        if it["status"] == "chyba":
            status = "Chyba – dvojklik"
        values = (it.get("title") or it["url"], it["source"], size,
                  _bar(frac) if it["status"] != "caka" or frac else "",
                  human_speed(it.get("speed")) if it["status"] == "stahuje" else "", status)
        if self.tv.exists(iid):
            self.tv.item(iid, values=values, tags=(it["status"],))
        else:
            order = self.dq.ids()
            before = [i for i in order[:order.index(iid)] if self.tv.exists(i)] if iid in order else []
            index = self.tv.index(before[-1]) + 1 if before else 0
            self.tv.insert("", index, iid=iid, values=values, tags=(it["status"],))

    def _update_summary(self):
        c = self.dq.counts()
        parts = []
        for key, word in (("stahuje", "sťahuje sa"), ("caka", "čaká"), ("pauza", "pozastavené"),
                          ("chyba", "chyby"), ("hotovo", "hotovo")):
            if c.get(key):
                parts.append(f"{word}: {c[key]}")
        if c.get("speed"):
            parts.append(human_speed(c["speed"]))
        self.summary_var.set("  ·  ".join(parts))
        if self.tv.get_children():
            self.empty_lbl.place_forget()
        else:
            self.empty_lbl.place(relx=0.5, rely=0.45, anchor="center")

    def on_events(self, ids: set):
        order = {iid: n for n, iid in enumerate(self.dq.ids())}
        for iid in sorted(ids, key=lambda i: order.get(i, -1)):
            self.refresh(iid)
        self._update_summary()


class ScanDialog:
    """Okno „Prehľadať stránku“ – ukáže všetky videá, streamy a súbory nájdené na stránke."""

    COLS = (("kind", "Typ", 90, "w"), ("provider", "Zdroj", 110, "w"), ("title", "Názov", 260, "w"),
            ("url", "Adresa", 330, "w"))

    def __init__(self, win, url: str = ""):
        self.win, self.root, self.t = win, win.root, win.theme
        self.found: list = []
        self.q: queue.Queue = queue.Queue()
        top = self.top = tk.Toplevel(self.root)
        top.title("Prehľadať stránku – Kometa")
        top.configure(bg=BG)
        top.transient(self.root)
        top.minsize(760, 480)
        wrap = ttk.Frame(top, padding=(18, 14, 18, 14))
        wrap.pack(fill="both", expand=True)
        ttk.Label(wrap, text="Nájdi video na akejkoľvek stránke", font=self.t.f_head).pack(anchor="w")
        ttk.Label(wrap, style="Muted.TLabel", wraplength=820, justify="left",
                  text="Vlož adresu stránky (kurz, článok, stránka s vloženým prehrávačom Vimeo, YouTube, Wistia…). "
                       "Kometa prejde stránku aj vnorené rámčeky a ukáže, čo našla. Pri stránkach po prihlásení "
                       "nastav prehliadač v karte „Účty a nastavenia“.").pack(anchor="w", pady=(2, 10))
        row = ttk.Frame(wrap)
        row.pack(fill="x")
        self.url_var = tk.StringVar(value=url)
        e = ttk.Entry(row, textvariable=self.url_var)
        e.pack(side="left", fill="x", expand=True)
        e.bind("<Return>", lambda ev: self.scan())
        self.scan_btn = ttk.Button(row, text="Prehľadať", style="Soft.TButton", command=self.scan)
        self.scan_btn.pack(side="left", padx=(8, 0))
        self.status = tk.StringVar(value="")
        ttk.Label(wrap, textvariable=self.status, style="Muted.TLabel").pack(anchor="w", pady=(8, 4))

        ttk.Label(wrap, style="Muted.TLabel", text="Ctrl/Shift + klik = výber viacerých. Odporúčané sú vybrané "
                                                   "vopred, sivé sú menej pravdepodobné (odkazy, reklamy, súbory)."
                  ).pack(side="bottom", anchor="w", pady=(6, 0))
        bottom = ttk.Frame(wrap)
        bottom.pack(side="bottom", fill="x", pady=(10, 0))
        box = tk.Frame(wrap, bg=CARD, highlightthickness=1, highlightbackground=BORDER)
        box.pack(fill="both", expand=True)
        self.tv = ttk.Treeview(box, columns=[c[0] for c in self.COLS], show="headings", selectmode="extended")
        for key, text, width, anchor in self.COLS:
            self.tv.heading(key, text=text, anchor=anchor)
            self.tv.column(key, width=width, anchor=anchor, stretch=key in ("title", "url"))
        sb = ttk.Scrollbar(box, orient="vertical", command=self.tv.yview)
        self.tv.configure(yscrollcommand=sb.set)
        self.tv.pack(side="left", fill="both", expand=True)
        sb.pack(side="right", fill="y")
        self.tv.tag_configure("low", foreground=MUTED)
        self.tv.bind("<<TreeviewSelect>>", lambda ev: self._update_btn())

        ttk.Label(bottom, text="Heslo k videu (ak je chránené):").pack(side="left")
        self.pw_var = tk.StringVar()
        ttk.Entry(bottom, textvariable=self.pw_var, show="•", width=18).pack(side="left", padx=(8, 0))
        ttk.Button(bottom, text="Zavrieť", command=top.destroy).pack(side="right")
        self.go = ttk.Button(bottom, text="Stiahnuť vybrané", style="Accent.TButton", command=self.download,
                             state="disabled")
        self.go.pack(side="right", padx=(0, 8))
        w, h = 900, 600
        x = self.root.winfo_rootx() + max((self.root.winfo_width() - w) // 2, 0)
        y = self.root.winfo_rooty() + 60
        top.geometry(f"{w}x{h}+{x}+{y}")
        e.focus_set()
        if url:
            top.after(150, self.scan)

    def scan(self):
        url = self.url_var.get().strip()
        if not re.match(r"^https?://", url):
            self.status.set("Zadaj adresu stránky (začína na http:// alebo https://).")
            return
        self.scan_btn.configure(state="disabled")
        self.status.set("Prehľadávam stránku… (môže to trvať pár sekúnd)")
        for i in self.tv.get_children():
            self.tv.delete(i)

        def work():
            try:
                self.q.put(("ok", skener.Skener(log=lambda m: None).scan(url)))
            except Exception as e:  # noqa: BLE001
                self.q.put(("err", clean_error(e)))

        threading.Thread(target=work, daemon=True).start()
        self.top.after(150, self._poll)

    def _poll(self):
        try:
            kind, data = self.q.get_nowait()
        except queue.Empty:
            if self.top.winfo_exists():
                self.top.after(150, self._poll)
            return
        self.scan_btn.configure(state="normal")
        if kind == "err":
            self.status.set("✖ " + data)
            return
        self.found = [n for n in data if n.kind != "stranka"]
        best = {id(n) for n in skener.best_candidates(self.found)}
        for idx, n in enumerate(self.found):
            tags = () if n.score >= 50 else ("low",)
            title = n.title or "—"
            if n.note:
                title += f"  ({n.note})"
            self.tv.insert("", "end", iid=str(idx), tags=tags,
                           values=(skener.KIND_TEXT.get(n.kind, n.kind), n.provider, title, n.url))
            if id(n) in best:
                self.tv.selection_add(str(idx))
        self.status.set(f"Nájdené: {len(self.found)} · odporúčané: {len(best)}" if self.found
                        else "Na stránke sa nič nenašlo. Ak je obsah len po prihlásení, nastav prehliadač "
                             "v karte „Účty a nastavenia“.")
        self._update_btn()

    def _update_btn(self):
        n = len(self.tv.selection())
        self.go.configure(state="normal" if n else "disabled", text=f"Stiahnuť vybrané ({n})" if n
                          else "Stiahnuť vybrané")

    def download(self):
        picks = [self.found[int(i)] for i in self.tv.selection()]
        if not picks:
            return
        d = self.win.downloads
        outdir = self.win.outdir_var.get().strip() or str(default_outdir())
        for n in picks:
            self.win.dq.add([n.url], d._preset(), outdir, referer=n.referer, password=self.pw_var.get() or None,
                            titles=[n.title], providers=[n.provider], from_scan=True)
        self.win.set_status(f"Pridané do frontu: {len(picks)}")
        self.win.nb.select(0)
        self.top.destroy()


class PlaylistDialog:
    """Okno „Playlist / kanál“ – zoznam videí na výber (všetky, najnovšie, podľa filtra)."""

    COLS = (("n", "#", 46, "e"), ("title", "Názov", 470, "w"), ("dur", "Dĺžka", 80, "e"), ("date", "Dátum", 96, "w"))

    def __init__(self, win, url: str = ""):
        self.win, self.root, self.t = win, win.root, win.theme
        self.info: dict | None = None
        self.shown: list[int] = []
        self.q: queue.Queue = queue.Queue()
        top = self.top = tk.Toplevel(self.root)
        top.title("Playlist / kanál – Kometa")
        top.configure(bg=BG)
        top.transient(self.root)
        top.minsize(760, 520)
        wrap = ttk.Frame(top, padding=(18, 14, 18, 14))
        wrap.pack(fill="both", expand=True)
        ttk.Label(wrap, text="Stiahnuť playlist alebo kanál", font=self.t.f_head).pack(anchor="w")
        ttk.Label(wrap, style="Muted.TLabel", wraplength=820, justify="left",
                  text="Vlož odkaz na playlist (youtube.com/playlist?list=…) alebo kanál (youtube.com/@meno). "
                       "Zobrazí sa zoznam videí – vyber, ktoré chceš stiahnuť.").pack(anchor="w", pady=(2, 10))
        row = ttk.Frame(wrap)
        row.pack(fill="x")
        self.url_var = tk.StringVar(value=url)
        e = ttk.Entry(row, textvariable=self.url_var)
        e.pack(side="left", fill="x", expand=True)
        e.bind("<Return>", lambda ev: self.load())
        self.tab_var = tk.StringVar(value="Videá")
        self.tab_cb = ttk.Combobox(row, textvariable=self.tab_var, values=["Videá", "Shorts", "Naživo"],
                                   state="readonly", width=8)
        self.tab_cb.pack(side="left", padx=(8, 0))
        self.load_btn = ttk.Button(row, text="Načítať", style="Soft.TButton", command=self.load)
        self.load_btn.pack(side="left", padx=(8, 0))
        self.status = tk.StringVar(value="")
        ttk.Label(wrap, textvariable=self.status, style="Muted.TLabel").pack(anchor="w", pady=(8, 4))

        tools = ttk.Frame(wrap)
        tools.pack(fill="x", pady=(0, 6))
        ttk.Label(tools, text="Filter:").pack(side="left")
        self.filter_var = tk.StringVar()
        fe = ttk.Entry(tools, textvariable=self.filter_var, width=24)
        fe.pack(side="left", padx=(6, 14))
        self.filter_var.trace_add("write", lambda *_: self._fill())
        ttk.Button(tools, text="Všetky", style="Link.TButton", command=self.select_all).pack(side="left")
        ttk.Button(tools, text="Žiadne", style="Link.TButton", command=self.select_none).pack(side="left", padx=(4, 0))
        ttk.Label(tools, text="Najnovších:").pack(side="left", padx=(14, 4))
        self.n_var = tk.IntVar(value=10)
        ttk.Spinbox(tools, from_=1, to=5000, textvariable=self.n_var, width=6).pack(side="left")
        ttk.Button(tools, text="Vybrať", style="Link.TButton", command=self.select_newest).pack(side="left", padx=(4, 0))

        ttk.Label(wrap, style="Muted.TLabel", text="Ctrl/Shift + klik = výber viacerých. Len na súkromné použitie."
                  ).pack(side="bottom", anchor="w", pady=(6, 0))
        bottom = ttk.Frame(wrap)
        bottom.pack(side="bottom", fill="x", pady=(10, 0))
        self.folder_var = tk.BooleanVar(value=True)
        ttk.Checkbutton(bottom, text="Uložiť do priečinka s názvom playlistu", variable=self.folder_var,
                        style="Plain.TCheckbutton").pack(side="left")
        ttk.Button(bottom, text="Zavrieť", command=top.destroy).pack(side="right")
        self.go = ttk.Button(bottom, text="Stiahnuť vybrané", style="Accent.TButton", command=self.download,
                             state="disabled")
        self.go.pack(side="right", padx=(0, 8))

        box = tk.Frame(wrap, bg=CARD, highlightthickness=1, highlightbackground=BORDER)
        box.pack(fill="both", expand=True)
        self.tv = ttk.Treeview(box, columns=[c[0] for c in self.COLS], show="headings", selectmode="extended")
        for key, text, width, anchor in self.COLS:
            self.tv.heading(key, text=text, anchor=anchor)
            self.tv.column(key, width=width, anchor=anchor, stretch=key == "title")
        sb = ttk.Scrollbar(box, orient="vertical", command=self.tv.yview)
        self.tv.configure(yscrollcommand=sb.set)
        self.tv.pack(side="left", fill="both", expand=True)
        sb.pack(side="right", fill="y")
        self.tv.bind("<<TreeviewSelect>>", lambda ev: self._update_btn())

        w, h = 900, 640
        x = self.root.winfo_rootx() + max((self.root.winfo_width() - w) // 2, 0)
        y = self.root.winfo_rooty() + 40
        top.geometry(f"{w}x{h}+{x}+{y}")
        e.focus_set()
        if url:
            top.after(150, self.load)

    def load(self):
        url = self.url_var.get().strip()
        if not re.match(r"^https?://", url):
            self.status.set("Zadaj odkaz na playlist alebo kanál (začína na http:// alebo https://).")
            return
        tab = {"Videá": "videos", "Shorts": "shorts", "Naživo": "streams"}[self.tab_var.get()]
        self.load_btn.configure(state="disabled")
        self.status.set("Načítavam zoznam videí… (pri veľkom kanáli to môže trvať dlhšie)")
        for i in self.tv.get_children():
            self.tv.delete(i)

        def work():
            try:
                self.q.put(("ok", media.list_collection(media.collection_url(url, tab), log=lambda m: None)))
            except Exception as e:  # noqa: BLE001
                self.q.put(("err", clean_error(e)))

        threading.Thread(target=work, daemon=True).start()
        self.top.after(150, self._poll)

    def _poll(self):
        try:
            kind, data = self.q.get_nowait()
        except queue.Empty:
            if self.top.winfo_exists():
                self.top.after(150, self._poll)
            return
        self.load_btn.configure(state="normal")
        if kind == "err":
            self.status.set("✖ " + data.splitlines()[0][:200])
            return
        self.info = data
        self._fill()
        self.select_all()

    def _fill(self):
        if not self.info:
            return
        flt = self.filter_var.get().strip().lower()
        for i in self.tv.get_children():
            self.tv.delete(i)
        self.shown = []
        for n, e in enumerate(self.info["entries"]):
            if flt and flt not in (e["title"] or "").lower():
                continue
            d = e.get("date") or ""
            date = f"{d[6:8]}.{d[4:6]}.{d[:4]}" if len(d) == 8 else ""
            self.tv.insert("", "end", iid=str(n), values=(n + 1, e["title"], format_duration(e.get("duration")), date))
            self.shown.append(n)
        total = sum(e.get("duration") or 0 for e in self.info["entries"])
        who = f" · {self.info['uploader']}" if self.info.get("uploader") and self.info["uploader"] not in self.info["title"] else ""
        self.status.set(f"{self.info['title']}{who} · videí: {len(self.info['entries'])}"
                        + (f" · spolu {format_duration(total)}" if total else "")
                        + (f" · zobrazené: {len(self.shown)}" if flt else ""))
        self._update_btn()

    def select_all(self):
        self.tv.selection_set([str(n) for n in self.shown])

    def select_none(self):
        self.tv.selection_set(())

    def select_newest(self):
        try:
            k = max(1, int(self.n_var.get()))
        except (tk.TclError, ValueError):
            k = 10
        self.tv.selection_set([str(n) for n in self.shown[:k]])   # playlist/kanál je zoradený od najnovších

    def _update_btn(self):
        n = len(self.tv.selection())
        self.go.configure(state="normal" if n else "disabled",
                          text=f"Stiahnuť vybrané ({n})" if n else "Stiahnuť vybrané")

    def download(self):
        if not self.info:
            return
        picks = [self.info["entries"][int(i)] for i in sorted(self.tv.selection(), key=int)]
        if not picks:
            return
        outdir = self.win.outdir_var.get().strip() or str(default_outdir())
        if self.folder_var.get():
            outdir = str(Path(outdir) / safe_name(self.info["title"], 100))
        new = self.win.dq.add([e["url"] for e in picks], self.win.downloads._preset(), outdir,
                              titles=[e["title"] for e in picks])
        self.win.set_status(f"Pridané do frontu: {len(new)} z {len(picks)}")
        self.win.nb.select(0)
        self.top.destroy()


class AccountsTab:
    """Karta „Účty a nastavenia“ – Webshare, Fastshare, súbežné sťahovanie, cookies."""

    BROWSERS = ["Nepoužívať", "Firefox", "Chrome", "Edge", "Brave", "Opera", "Vivaldi"]

    def __init__(self, win, parent):
        self.win = win
        self.root = win.root
        self.t = win.theme
        self.cfg = win.cfg
        self.q: queue.Queue = queue.Queue()
        self.rows: dict[str, dict] = {}
        self._build(parent)
        self.root.after(150, self._poll)
        self.root.after(800, self.check_all)

    def _build(self, parent):
        wrap = ttk.Frame(parent, padding=(18, 14, 18, 12))
        wrap.pack(fill="both", expand=True)
        for n, (service, title, hint) in enumerate((
                ("webshare", "Webshare.cz", "Prihlasovacie meno alebo e-mail a heslo k Webshare."),
                ("fastshare", "Fastshare.cz", "Prihlasovacie meno a heslo k Fastshare."))):
            card, body = self.t.card(wrap, str(n + 1), title)
            card.pack(fill="x", pady=(0, 10))
            user_var = tk.StringVar(value=self.cfg.get(f"{service}_user", ""))
            pw_var = tk.StringVar(value="")
            grid = ttk.Frame(body, style="Card.TFrame")
            grid.pack(fill="x")
            ttk.Label(grid, text="Meno:", style="Card.TLabel").grid(row=0, column=0, sticky="w", padx=(0, 8))
            ttk.Entry(grid, textvariable=user_var, width=28).grid(row=0, column=1, sticky="w")
            ttk.Label(grid, text="Heslo:", style="Card.TLabel").grid(row=0, column=2, sticky="w", padx=(18, 8))
            pw = ttk.Entry(grid, textvariable=pw_var, width=22, show="•")
            pw.grid(row=0, column=3, sticky="w")
            login = ttk.Button(grid, text="Prihlásiť", style="Soft.TButton",
                               command=lambda s=service: self.login(s))
            login.grid(row=0, column=4, padx=(12, 0))
            ttk.Button(grid, text="Odhlásiť", command=lambda s=service: self.logout(s)).grid(row=0, column=5,
                                                                                         padx=(6, 0))
            pw.bind("<Return>", lambda e, s=service: self.login(s))
            status = ttk.Label(body, text=hint, style="Muted.Card.TLabel")
            status.pack(anchor="w", pady=(8, 0))
            self.rows[service] = {"user": user_var, "pw": pw_var, "status": status, "btn": login}

        card, body = self.t.card(wrap, "3", "Nastavenia")
        card.pack(fill="x")
        r = ttk.Frame(body, style="Card.TFrame")
        r.pack(fill="x")
        ttk.Label(r, text="Súčasne sťahovať:", style="Card.TLabel").pack(side="left")
        self.par_var = tk.IntVar(value=int(self.cfg.get("parallel") or 2))
        sp = ttk.Spinbox(r, from_=1, to=6, textvariable=self.par_var, width=4, command=self._save_settings)
        sp.pack(side="left", padx=(8, 4))
        sp.bind("<FocusOut>", lambda e: self._save_settings())
        ttk.Label(r, text="súbory naraz", style="Muted.Card.TLabel").pack(side="left")
        ttk.Label(r, text="Spojenia na jeden súbor:", style="Card.TLabel").pack(side="left", padx=(28, 0))
        self.conn_var = tk.IntVar(value=int(self.cfg.get("connections") or 4))
        sp2 = ttk.Spinbox(r, from_=1, to=16, textvariable=self.conn_var, width=4, command=self._save_settings)
        sp2.pack(side="left", padx=(8, 4))
        sp2.bind("<FocusOut>", lambda e: self._save_settings())
        ttk.Label(r, text="(rýchlejšie veľké súbory a streamy)", style="Muted.Card.TLabel").pack(side="left")

        ttk.Checkbutton(body, text="Kompatibilný zvuk vo videách – AAC (predvolené, prehrá každý prehrávač) + Opus "
                                   "v jednom MKV", variable=self.win.compat_var).pack(anchor="w", pady=(8, 0))
        self.auto_var = tk.BooleanVar(value=bool(self.cfg.get("autoresume", True)))
        ttk.Checkbutton(body, text="Po spustení programu automaticky pokračovať v nedokončených sťahovaniach",
                        variable=self.auto_var, command=self._save_settings).pack(anchor="w", pady=(8, 0))

        r2 = ttk.Frame(body, style="Card.TFrame")
        r2.pack(fill="x", pady=(8, 0))
        ttk.Label(r2, text="Stránky po prihlásení – použiť prihlásenie z prehliadača:", style="Card.TLabel").pack(
            side="left")
        cur = (self.cfg.get("cookies_browser") or "").capitalize() or "Nepoužívať"
        self.browser_var = tk.StringVar(value=cur if cur in self.BROWSERS else "Nepoužívať")
        bc = ttk.Combobox(r2, textvariable=self.browser_var, values=self.BROWSERS, state="readonly", width=14)
        bc.pack(side="left", padx=(8, 0))
        bc.bind("<<ComboboxSelected>>", lambda e: self._save_settings())
        ttk.Label(body, style="Muted.Card.TLabel",
                  text="Pre Instagram, Facebook, platené kurzy či súkromné Vimeo: prihlás sa na stránke "
                       "v prehliadači a vyber ho tu – Kometa použije jeho prihlásenie (cookies). Najspoľahlivejší "
                       "je Firefox; Chrome a Edge treba pri sťahovaní zavrieť.", wraplength=820, justify="left").pack(anchor="w", pady=(6, 0))

        r3 = ttk.Frame(body, style="Card.TFrame")
        r3.pack(fill="x", pady=(10, 0))
        ttk.Label(r3, text="Priečinok:", style="Card.TLabel").pack(side="left")
        ttk.Entry(r3, textvariable=self.win.outdir_var).pack(side="left", fill="x", expand=True, padx=(8, 6))
        ttk.Button(r3, text="Zmeniť…", command=self.win.pick_outdir).pack(side="left")

        deps = check_dependencies()
        where = ("v Správcovi poverení Windows" if os.name == "nt" else "v systémovej kľúčenke") \
            if deps["keyring"] else f"v súbore {CONFIG_DIR} (zakódované)"
        ttk.Label(wrap, style="Muted.TLabel", text=f"Heslá sa ukladajú {where}, len na tomto počítači. "
                  "Program neobchádza limity bezplatných účtov.", wraplength=880, justify="left").pack(anchor="w", pady=(10, 0))

    def _save_settings(self):
        try:
            par = max(1, min(6, int(self.par_var.get())))
        except (tk.TclError, ValueError):
            par = 2
        try:
            conns = max(1, min(16, int(self.conn_var.get())))
        except (tk.TclError, ValueError):
            conns = 4
        b = self.browser_var.get()
        self.cfg.update(parallel=par, connections=conns, autoresume=bool(self.auto_var.get()),
                        cookies_browser="" if b == "Nepoužívať" else b.lower())
        save_config(self.cfg)
        apply_settings(self.cfg)
        self.win.dq.schedule()

    # ---------- účty ----------
    def _set_status(self, service: str, text: str, ok: bool | None):
        style = {True: "Ok.Card.TLabel", False: "Err.Card.TLabel", None: "Muted.Card.TLabel"}[ok]
        self.rows[service]["status"].configure(text=text, style=style)

    def login(self, service: str):
        row = self.rows[service]
        user, pw = row["user"].get().strip(), row["pw"].get()
        if not user:
            messagebox.showinfo(APP_NAME, "Zadaj prihlasovacie meno.")
            return
        pw = pw or get_password(service, user) or ""
        if not pw:
            messagebox.showinfo(APP_NAME, "Zadaj heslo.")
            return
        row["btn"].configure(state="disabled")
        self._set_status(service, "Prihlasujem…", None)

        def work():
            try:
                if service == "webshare":
                    client = WebshareClient()
                    token = client.login(user, pw)
                    info = client.user_info()
                    self.q.put(("ok", service, user, pw, info, token))
                else:
                    client = FastshareClient()
                    client.login(user, pw)
                    info = client.user_info()
                    self.q.put(("ok", service, user, pw, info, None))
            except Exception as e:  # noqa: BLE001
                self.q.put(("err", service, clean_error(e)))

        threading.Thread(target=work, daemon=True).start()

    def logout(self, service: str):
        user = self.cfg.get(f"{service}_user") or self.rows[service]["user"].get().strip()
        if user:
            delete_password(service, user)
        self.cfg[f"{service}_user"] = ""
        if service == "webshare":
            self.cfg["webshare_token"] = ""
        save_config(self.cfg)
        self.win.accounts.reset(service)
        self.rows[service]["pw"].set("")
        self._set_status(service, "Odhlásený.", None)

    def check_all(self):
        for service in ("webshare", "fastshare"):
            user = self.cfg.get(f"{service}_user")
            if user and get_password(service, user):
                self._set_status(service, "Overujem účet…", None)

                def work(s=service):
                    try:
                        client = self.win.accounts.webshare() if s == "webshare" else self.win.accounts.fastshare()
                        self.q.put(("info", s, client.user_info()))
                    except Exception as e:  # noqa: BLE001
                        self.q.put(("err", s, clean_error(e)))

                threading.Thread(target=work, daemon=True).start()

    def _poll(self):
        try:
            while True:
                msg = self.q.get_nowait()
                kind, service = msg[0], msg[1]
                self.rows[service]["btn"].configure(state="normal")
                if kind == "ok":
                    _, _, user, pw, info, token = msg
                    set_password(service, user, pw)
                    self.cfg[f"{service}_user"] = user
                    if token:
                        self.cfg["webshare_token"] = token
                    save_config(self.cfg)
                    self.win.accounts.reset(service)
                    self.rows[service]["pw"].set("")
                    self._set_status(service, f"✔ Prihlásený ako {user} – {info['text']}", info["vip"])
                    self.win.dq.start([i for i in self.win.dq.ids()
                                       if (self.win.dq.snapshot(i) or {}).get("status") == "chyba"
                                       and (self.win.dq.snapshot(i) or {}).get("source", "").lower() == service])
                elif kind == "info":
                    info = msg[2]
                    user = self.cfg.get(f"{service}_user")
                    self._set_status(service, f"✔ {user} – {info['text']}", info["vip"])
                else:
                    self._set_status(service, "✖ " + msg[2], False)
        except queue.Empty:
            pass
        self.root.after(150, self._poll)


class MainWindow:
    def __init__(self):
        self.root = tk.Tk()
        self.root.title(f"{APP_NAME} – sťahovanie videí a súborov")
        self.root.configure(bg=BG)
        self.root.minsize(860, 760)
        self.theme = Theme(self.root)
        _extra_styles(self.theme)

        self.cfg = load_config()
        apply_settings(self.cfg)
        self.accounts = Accounts(self.cfg)
        self.events: queue.Queue = queue.Queue()
        self.dq = DownloadQueue(self.cfg, self.accounts, emit=lambda iid: self.events.put(iid),
                                log=lambda m: None)
        self.outdir_var = tk.StringVar(value=self.cfg.get("outdir") or str(default_outdir()))
        self.outdir_var.trace_add("write", lambda *_: self._outdir_changed())
        self.compat_var = tk.BooleanVar(value=self.cfg.get("audio_mode", "oboje") != "opus")
        self.compat_var.trace_add("write", lambda *_: self._compat_changed())

        self._build()
        self._center(960, 860)
        self.root.protocol("WM_DELETE_WINDOW", self.on_close)
        self.root.after(200, self._poll)
        self.root.after(400, self._startup_checks)
        self.root.after(600, self.dq.schedule)

    def _build(self):
        head = ttk.Frame(self.root, padding=(22, 14, 22, 4))
        head.pack(fill="x")
        big = float(self.root.tk.call("tk", "scaling")) > 1.6
        self._logo = tk.PhotoImage(data=_LOGO["56" if big else "40"])
        self._icon = tk.PhotoImage(data=_LOGO["64"])
        self.root.iconphoto(True, self._icon)
        tk.Label(head, image=self._logo, bg=BG, bd=0).pack(side="left", padx=(0, 12))
        tbox = ttk.Frame(head)
        tbox.pack(side="left")
        ttk.Label(tbox, text=APP_NAME, font=self.theme.f_title).pack(anchor="w")
        ttk.Label(tbox, text="YouTube, Facebook, Instagram, TikTok, Webshare, Fastshare – s pokračovaním "
                             "prerušeného sťahovania.", foreground=MUTED, font=self.theme.f_sub).pack(anchor="w")
        self.status_var = tk.StringVar(value="")
        ttk.Label(self.root, textvariable=self.status_var, style="Warn.TLabel",
                  padding=(24, 2, 24, 6)).pack(side="bottom", fill="x")

        self.nb = ttk.Notebook(self.root)
        self.nb.pack(fill="both", expand=True)
        tabs = []
        for title in ("⬇  Sťahovanie", "♪  Jazykové stopy (YouTube)", "⚙  Účty a nastavenia"):
            f = ttk.Frame(self.nb)
            self.nb.add(f, text=title)
            tabs.append(f)
        self.downloads = DownloadsTab(self, tabs[0])
        self.languages = LanguagesTab(self, tabs[1])
        self.accounts_tab = AccountsTab(self, tabs[2])
        self.nb.bind("<<NotebookTabChanged>>", self._tab_changed)

    def _tab_changed(self, _e=None):
        if self.nb.index("current") == 1:
            self.languages.prefill()

    def _center(self, w, h):
        self.root.update_idletasks()
        sw, sh = self.root.winfo_screenwidth(), self.root.winfo_screenheight()
        w, h = min(w, sw - 40), min(h, sh - 80)
        self.root.geometry(f"{w}x{h}+{(sw - w) // 2}+{max((sh - h) // 3, 0)}")

    # ---------- spoločné ----------
    def set_status(self, text: str):
        self.status_var.set(text)
        self.root.after(6000, lambda: self.status_var.get() == text and self.status_var.set(""))

    def _compat_changed(self):
        self.cfg["audio_mode"] = "oboje" if self.compat_var.get() else "opus"
        save_config(self.cfg)
        apply_settings(self.cfg)
        if hasattr(self, "languages"):
            self.languages._update_order()

    def _outdir_changed(self):
        self.cfg["outdir"] = self.outdir_var.get().strip()
        save_config(self.cfg)

    def pick_outdir(self):
        d = filedialog.askdirectory(title="Kam ukladať súbory?", mustexist=False,
                                    initialdir=self.outdir_var.get() or str(Path.home()))
        if d:
            self.outdir_var.set(str(Path(d)))

    def open_outdir(self):
        d = Path(self.outdir_var.get().strip() or default_outdir())
        d.mkdir(parents=True, exist_ok=True)
        open_folder(d)

    def _poll(self):
        ids = set()
        try:
            while True:
                ids.add(self.events.get_nowait())
        except queue.Empty:
            pass
        if ids:
            self.downloads.on_events(ids)
        self.root.after(250, self._poll)

    def _startup_checks(self):
        deps = check_dependencies()
        if not deps["yt-dlp"]:
            messagebox.showerror(APP_NAME, "Chýba knižnica yt-dlp.\n\nSpusti Instalacia.bat, alebo v príkazovom "
                                           'riadku:\n  py -m pip install -U -r requirements.txt')
        missing = [n for n, k in (("FFmpeg", "ffmpeg"), ("Deno", "js")) if not deps[k]]
        if missing:
            self.status_var.set("⚠ Chýba: " + ", ".join(missing) + " – spusti Instalacia.bat")

    def on_close(self):
        if self.dq.running() or self.languages.busy:
            if not messagebox.askyesno(APP_NAME, "Práve sa sťahuje.\n\nZavrieť program? Rozpracované súbory "
                                                 "ostanú a sťahovanie bude pokračovať pri ďalšom spustení."):
                return
        self.status_var.set("Ukladám rozpracované sťahovania…")
        self.root.update()
        self.languages.cancel.set()
        self.dq.shutdown()
        self.root.destroy()

    def run(self):
        self.root.mainloop()


def main():
    if os.name == "nt":
        try:
            import ctypes
            ctypes.windll.shcore.SetProcessDpiAwareness(1)
        except Exception:
            pass
    MainWindow().run()
