// Makes a small test data file: UndertaleModCli new -o data.win; UndertaleModCli load data.win -s make_fixture.csx -o fixture.win
using System.Linq;
using UndertaleModLib.Compiler;
using UndertaleModLib.Models;

var g = new CodeImportGroup(Data);
g.QueueReplace("gml_Script_msgsetloc", "return argument1;");
g.QueueReplace("gml_Script_scr_speaker", "global.speaker = argument0;");
// Deltarune names that wave objects use (bullet patterns).
g.QueueReplace("gml_Script_camerax", "return 0;");
g.QueueReplace("gml_Script_cameray", "return 0;");
g.QueueReplace("gml_Script_instance_create", "return instance_create_depth(argument0, argument1, 0, argument2);");
g.QueueReplace("gml_Object_obj_collidebullet_Create_0", "damage = 1;");
g.QueueReplace("gml_Object_obj_heart_Create_0", "hp = 1;");
g.QueueReplace("gml_Object_obj_growtangle_Create_0", "image_xscale = 2;");
g.QueueReplace("gml_Object_obj_talk_Create_0", "scr_speaker(\"susie\");\nmsgsetloc(0, \"* Hey^1, Kris./%\", \"obj_talk_slash_Create_0_gml_2_0\");\nx = 10;");
g.Import();

var obj = Data.GameObjects.ByName("obj_talk");
var room = Data.Rooms[0];
room.GameObjects.Add(new UndertaleRoom.GameObject { X = 32, Y = 48, ObjectDefinition = obj, InstanceID = Data.GeneralInfo.LastObj++ });
