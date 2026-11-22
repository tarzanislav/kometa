@echo off
title Kometa - instalacia
cd /d "%~dp0"
echo.
echo  ==========================================================
echo   KOMETA - instalacia potrebnych programov
echo   (Python, FFmpeg, Deno, yt-dlp) a zastupca na plochu
echo  ==========================================================
echo.
where winget >nul 2>nul || (
  echo  Na tomto pocitaci chyba "winget" ^(Instalator aplikacii^).
  echo  Nainstaluj z Microsoft Store aplikaciu "App Installer"
  echo  a potom spusti tento subor znova.
  echo.
  pause
  exit /b 1
)
echo  [1/5] Python...
winget install -e --id Python.Python.3.13 --accept-source-agreements --accept-package-agreements
echo.
echo  [2/5] FFmpeg...
winget install -e --id Gyan.FFmpeg --accept-source-agreements --accept-package-agreements
echo.
echo  [3/5] Deno (YouTube ho dnes vyzaduje)...
winget install -e --id DenoLand.Deno --accept-source-agreements --accept-package-agreements
echo.
echo  [4/5] Kniznice (yt-dlp, requests, keyring)...
call :findpy
if not defined PY (
  echo.
  echo  Python sa prave nainstaloval. ZATVOR toto okno a spusti
  echo  Instalacia.bat este raz - dokoncia sa posledne kroky.
  echo.
  pause
  exit /b 0
)
%PY% -m pip install --disable-pip-version-check -U -r "%~dp0requirements.txt" || (
  echo  Okno aplikacie ^(pywebview^) sa nenainstalovalo - Kometa sa otvori v okne prehliadaca Edge.
  %PY% -m pip install --disable-pip-version-check -U "yt-dlp[default]" requests keyring
)
echo.
echo  [5/5] Zastupca "Kometa" na ploche...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$d=[Environment]::GetFolderPath('Desktop'); $s=(New-Object -ComObject WScript.Shell).CreateShortcut(\"$d\Kometa.lnk\"); $s.TargetPath='%~dp0Spustit.bat'; $s.WorkingDirectory='%~dp0'; $s.IconLocation='%~dp0assets\kometa.ico'; $s.WindowStyle=7; $s.Description='Kometa - stahovanie videi a suborov'; $s.Save()"
echo.
echo  Hotovo! Kometu spustis ikonou na ploche alebo cez Spustit.bat.
echo.
pause
exit /b 0

:findpy
set "PY="
where py >nul 2>nul && (set "PY=py -3" & exit /b 0)
where python >nul 2>nul && python -c "import sys" >nul 2>nul && set "PY=python"
exit /b 0
