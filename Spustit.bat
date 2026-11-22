@echo off
title Kometa
cd /d "%~dp0"
call :findpy
if not defined PY (
  echo Python nie je nainstalovany. Spusti najprv Instalacia.bat.
  echo.
  pause
  exit /b 1
)
echo Kontrolujem aktualizaciu yt-dlp (YouTube a ostatne stranky sa casto menia)...
%PY% -m pip install -q --disable-pip-version-check -U "yt-dlp[default]" 2>nul
start "" %PYW% "%~dp0app.py"
exit /b 0

:findpy
set "PY="
set "PYW="
where py >nul 2>nul && (set "PY=py -3" & set "PYW=pyw -3" & exit /b 0)
where python >nul 2>nul && python -c "import sys" >nul 2>nul && (set "PY=python" & set "PYW=pythonw")
exit /b 0
