/*
 Port of byuu's color correction shader as described in https://byuu.net/video/color-emulation
 */

precision mediump float;

varying vec2 v_texCoord;
uniform sampler2D s_texture;

void main()
{
    float lcdGamma = 4.0, outGamma = 2.2;

    vec4 color = texture2D( s_texture, v_texCoord );

    color.rgb = pow(color.rgb, vec3(lcdGamma));
    gl_FragColor.r = pow((  0.0 * color.b +  50.0 * color.g + 255.0 * color.r) / 255.0, 1.0 / outGamma) * 255.0 / 280.0;
    gl_FragColor.g = pow(( 30.0 * color.b + 230.0 * color.g +  10.0 * color.r) / 255.0, 1.0 / outGamma) * 255.0 / 280.0;
    gl_FragColor.b = pow((220.0 * color.b +  10.0 * color.g +  50.0 * color.r) / 255.0, 1.0 / outGamma) * 255.0 / 280.0;
    gl_FragColor.a = 1.0;
}