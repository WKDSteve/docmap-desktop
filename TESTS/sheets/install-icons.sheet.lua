-- The hand check after the icons of the family changed: install docmap again and look at it where Windows shows it.
-- A Claude session may not install (it sees a redirected AppData and HKCU), so a person does it; the sheet asks.
-- Whether the installed program carries the current icon needs no eye: the second question runs the check of the
-- icon generator (the tool app-icons of the author's notes, icons.mjs exe), which compares the images byte for byte.
-- On 2026-11-14 the installed program still carried the icon of 2026-11-03 (the monogram dm, today doc).
--   testing sheets step install-icons
return {
	id = "docmap-desktop.install-icons",
	title = "docmap-desktop: installed again, the program, its links and its window show the current icon",
	task = "ALL/handlauf-hub-rules-und-docmap-neu-installieren-damit-program",
	engine = "manual",
	steps = {
		{ ask = "In a normal terminal (not a Claude session), scripts\\build-install.ps1 built and installed docmap?" },
		{
			ask = "node <app-icons>/icons.mjs exe"
				.. " $env:LOCALAPPDATA\\docmap-desktop\\docmap-desktop.exe docmap-desktop says: carries the current icon?",
		},
		{ ask = "The Start menu link docmap-desktop shows the docmap icon with the monogram doc?" },
		{
			ask = "The desktop link docmap-desktop shows the same icon, taken from the program"
				.. " (Properties, Change Icon names docmap-desktop.exe, no file below the repos)?",
		},
		{ ask = "The window of docmap and its taskbar button show the new icon?" },
		{
			ask = "Explorer shows the new icon on the program and the links"
				.. " (an old picture that sticks: ie4uinit -show, then look again)?",
		},
	},
}
