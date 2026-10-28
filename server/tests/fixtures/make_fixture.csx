// Makes a small test data file: UndertaleModCli new -o data.win; UndertaleModCli load data.win -s make_fixture.csx -o fixture.win
using System.Linq;
using UndertaleModLib.Compiler;
using UndertaleModLib.Models;

var g = new CodeImportGroup(Data);
g.QueueReplace("gml_Script_msgsetloc", "return argument1;");
g.QueueReplace("gml_Script_scr_speaker", "global.speaker = argument0;");
g.QueueReplace("gml_Object_obj_talk_Create_0", "scr_speaker(\"susie\");\nmsgsetloc(0, \"* Hey^1, Kris./%\", \"obj_talk_slash_Create_0_gml_2_0\");\nx = 10;");
g.Import();

var obj = Data.GameObjects.ByName("obj_talk");
var room = Data.Rooms[0];
room.GameObjects.Add(new UndertaleRoom.GameObject { X = 32, Y = 48, ObjectDefinition = obj, InstanceID = Data.GeneralInfo.LastObj++ });
