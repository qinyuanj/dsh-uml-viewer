@echo off
rem Thin Windows entry, installed in the examined project.
rem Same job as the sh entry `uml`: find Python 3.11+, forward every argument.
rem cmd re-parses %%*: an argument holding ^ & | < > %% or " cannot reach
rem Python intact (README, known limits). Keep this file ASCII and CRLF.
setlocal
set "UML_ROOT=%~dp0."
set "UML_TOOL=%UML_VIEWER_TOOL_ROOT%"
if "%UML_TOOL%"=="" set "UML_TOOL=%~dp0.uml-viewer\uml-viewer"
set "UML_PY_ARGS="
if not "%UML_PYTHON_EXECUTABLE%"=="" goto configured
where py >nul 2>nul && goto launcher
where python >nul 2>nul && goto plain
goto missing
:configured
set "UML_PY=%UML_PYTHON_EXECUTABLE%"
if exist "%UML_PY%" goto run
where "%UML_PY%" >nul 2>nul && goto run
goto missing
:launcher
set "UML_PY=py"
set "UML_PY_ARGS=-3"
goto run
:plain
set "UML_PY=python"
goto run
:missing
echo uml: native Python 3.11+ is required (UML_PYTHON_EXECUTABLE, py -3 or python). 1>&2
exit /b 1
:run
"%UML_PY%" %UML_PY_ARGS% -I -B "%UML_TOOL%\scripts\uml_posix.py" command "%UML_ROOT%" %*
exit /b %ERRORLEVEL%
