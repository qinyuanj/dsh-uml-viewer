@echo off
rem Windows install entry; a standalone download bootstraps the same Python
rem implementation with curl.exe (Windows 10 1803+). No PowerShell (ADR 0010).
rem Same job as the sh entry `get-uml-viewer`. Keep this file ASCII and CRLF.
setlocal
set "UML_PY_ARGS="
if not "%UML_PYTHON_EXECUTABLE%"=="" goto configured
where py >nul 2>nul && goto launcher
where python >nul 2>nul && goto plain
goto missing
:configured
set "UML_PY=%UML_PYTHON_EXECUTABLE%"
if exist "%UML_PY%" goto found
where "%UML_PY%" >nul 2>nul && goto found
goto missing
:launcher
set "UML_PY=py"
set "UML_PY_ARGS=-3"
goto found
:plain
set "UML_PY=python"
goto found
:missing
echo get-uml-viewer: native Python 3.11+ is required (UML_PYTHON_EXECUTABLE, py -3 or python). 1>&2
exit /b 1
:found
if exist "%~dp0uml_posix.py" goto local
where curl.exe >nul 2>nul || goto nocurl
set "UML_REF=%UML_VIEWER_REF%"
if "%UML_REF%"=="" set "UML_REF=main"
set "UML_URL=%UML_VIEWER_INSTALLER_URL%"
if "%UML_URL%"=="" set "UML_URL=https://raw.githubusercontent.com/qinyuanj/uml-viewer/%UML_REF%/scripts/uml_posix.py"
set "UML_TMP=%TEMP%\get-uml-viewer-%RANDOM%%RANDOM%"
mkdir "%UML_TMP%" || exit /b 1
curl.exe -fsSL "%UML_URL%" -o "%UML_TMP%\uml_posix.py" || goto fetchfailed
"%UML_PY%" %UML_PY_ARGS% -I "%UML_TMP%\uml_posix.py" install %*
set "UML_CODE=%ERRORLEVEL%"
rmdir /s /q "%UML_TMP%"
exit /b %UML_CODE%
:fetchfailed
rmdir /s /q "%UML_TMP%"
exit /b 1
:nocurl
echo get-uml-viewer: the standalone bootstrap needs curl.exe; or run scripts\get-uml-viewer.cmd from a checkout. 1>&2
exit /b 1
:local
"%UML_PY%" %UML_PY_ARGS% -I "%~dp0uml_posix.py" install %*
exit /b %ERRORLEVEL%
