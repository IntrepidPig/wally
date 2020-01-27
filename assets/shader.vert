#version 450

layout(location = 0) in vec4 pos;
layout(location = 1) in vec4 col;

layout(location = 0) out vec4 vColor;

void main() {
	vColor = col;
	gl_Position = pos;
}